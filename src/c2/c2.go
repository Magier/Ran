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
	bus "github.com/Magier/Ran/internal/bus"
)

type C2Started struct {
}

func (c C2Started) MessageName() string {
	return "c2 started"
}
func (c C2Started) String() string {
	return "c2 started"
}

type Session struct {
	Id       string
	Hostname string
	Os       string
	User     string
}

type SessionStarted struct {
	Session Session
}

func (c SessionStarted) MessageName() string {
	return "session started"
}

func (c SessionStarted) String() string {
	return "Session started: " + c.Session.Id
}

func StartC2(ctx context.Context, mb bus.MessageBus) {
	// listeners := make(map[string]net.Listener)

	var wg sync.WaitGroup
	mb.Subscribe(domain.StartListener{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		cmd := event.(domain.StartListener)
		wg.Add(1)
		go func() {
			err := startListener(ctx, mb, cmd.Port)
			if err != nil {
				mb.Publish(domain.ErrorMsg{Level: domain.LevelError, Msg: err.Error()})
			}
			// TODO handle disconnecting listener
			wg.Done()
		}()
		// return startListener(ctx, mb, cmd.Port)
		return nil, nil
	})

	mb.Subscribe(domain.ExecCmd{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		cmd := event.(domain.ExecCmd)
		// check technique to execute CMD -> kubectl exec uses API
		// or shell listener?
		fmt.Println(".... ExecCmd is not implemented in C2")
		_ = cmd
		return nil, nil
	})

	err := mb.Publish(C2Started{})
	if err != nil {
		// panic(err)
		slog.Error("C2", "can't publish c2 started event:", err.Error())
	}
	// wg.Wait()
}

func startListener(ctx context.Context, bus bus.MessageBus, port uint) error {
	listener, err := net.Listen("tcp", ":"+strconv.FormatUint(uint64(port), 10))
	if err != nil {
		return fmt.Errorf("Unable to bind to port: %s", err)
	}
	defer listener.Close()

	err = bus.Publish(ListenerReady{Name: "listener", Port: port})
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
		slog.Error("Error receiving command response: " + err.Error())
		return "", err
	}
	return strings.Trim(s, "\n"), nil
}

func handleSession(ctx context.Context, bus bus.MessageBus, conn net.Conn, id string, cmds <-chan string, results chan<- string) {
	defer conn.Close()
	slog.Debug("C2", "", "Handling session")

	hostname, err := sendCommand(conn, "hostname")
	if err != nil {
		slog.Error("Error reading from connection: " + err.Error())
	}
	user, err := sendCommand(conn, "whoami")
	if err != nil {
		slog.Error("Error reading from connection: " + err.Error())
	}
	os, err := sendCommand(conn, "uname")
	if err != nil {
		slog.Error("Error reading from connection: " + err.Error())
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
