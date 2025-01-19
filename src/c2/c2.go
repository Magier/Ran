package c2

import (
	"bytes"
	"context"
	"fmt"
	"log/slog"
	"net"
	"os/exec"
	"strings"
	"sync"

	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
	k8s "github.com/Magier/Ran/k8sclient"
)

const BuiltInC2 = "builtin"

type C2Client interface {
	Connect(context.Context, bus.MessageBus) error
	Execute(domain.Command) (domain.Message, error)
	GetServerIp() net.IP
	GetName() string
	IsReady() bool
	SetReady(state bool) C2Client
}

type Session struct {
	Id         string
	Hostname   string
	Os         string
	OsVersion  string
	User       string
	RemoteAddr string
	IsRoot     bool
}

func StartC2(ctx context.Context, mb bus.MessageBus) {
	// listeners := make(map[string]net.Listener)
	// TODO start builtin C2 once an action demands it
	c2Clients := map[string]C2Client{
		BuiltInC2:  NewBuiltInServer(mb),
		SliverKind: CreateSliverClient("../sliver_cfg.json"),
	}

	mb.Subscribe(domain.C2Connected{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		ev := msg.(domain.C2Connected)
		client, ok := c2Clients[ev.Name]
		if ok {
			c2Clients[ev.Name] = client.SetReady(true)
		}
		return nil, fmt.Errorf("No suitable client found to update C2 state")
	})

	mb.Subscribe(domain.StartListener{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		cmd := msg.(domain.Command)
		client, ok := selectClient(c2Clients, cmd)
		if ok {
			return client.Execute(cmd)
		}
		return nil, fmt.Errorf("No suitable client found to start listener")
	})

	mb.Subscribe(domain.ExecTTP{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		cmd := msg.(domain.ExecTTP)
		// check technique to execute CMD -> kubectl exec uses API
		// or shell listener?
		switch ch := cmd.C2Channel.(type) {
		case domain.ImplantC2Channel:
			if c2, ok := c2Clients[cmd.C2Channel.GetKind()]; ok {
				return c2.Execute(cmd)
			}
		case domain.PodExecC2Channel:
			stdout, stderr, err := execKubectl(ctx, cmd)
			if err != nil {
				slog.Warn(err.Error())
			} else {
				msg, err := cmd.TTP.HandleResult(cmd.Target, stdout, stderr)
				if err != nil {
					msg = domain.TTPFailed{Id: cmd.TTP.ID, TTP: cmd.TTP, Reason: err.Error()}
				} else if msg == nil { // no handler -> try default handler for ExecTTP
					return handleExecTTPResult(cmd, stdout, stderr)
				}
				return msg, err
			}
		case nil:
			if cmd.TTP.Execute.Code != "" {
				err := executeCode(cmd.TTP.Execute)
				if err != nil {
					slog.Warn(err.Error())
				}
			} else {
				slog.Warn("Can't Exec TTP: no channel defined and no code provided!")
			}
		default:
			slog.Warn(fmt.Sprintf("Can't Exec TTP: unclear how to handle channel %v", ch))
		}
		return nil, nil
	})

	mb.Subscribe(domain.StartC2{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		cmd := msg.(domain.StartC2)
		client, ok := c2Clients[cmd.C2Name]
		if !ok {
			return nil, fmt.Errorf("'%s' is not a valid C2 server to connect to", cmd.C2Name)
		}
		go connectToC2(ctx, mb, client)
		return nil, nil
	})

	// TODO: send command after actually conncting to C2, with the right IP(s)

	var wg sync.WaitGroup
	for _, client := range c2Clients {
		go func() {
			wg.Add(1)
			defer wg.Done()
			connectToC2(ctx, mb, client)
		}()
	}
	wg.Wait()
}

func connectToC2(ctx context.Context, mb bus.MessageBus, c2client C2Client) {
	// successful C2Connect event is sent by the client itself
	err := c2client.Connect(ctx, mb)
	if err != nil {
		err = mb.Publish(C2ConnectFailed{
			Name:   c2client.GetName(),
			Reason: err.Error(),
		})
		if err != nil {
			slog.Error(err.Error())
		}
	}
}

func selectClient(clients map[string]C2Client, msg domain.Command) (C2Client, bool) {
	var server string
	switch cmd := msg.(type) {
	case domain.StartListener:
		server = cmd.Server
	case domain.StopListener:
		server = cmd.Server
	}

	// no server defined means the C2 will choose the best option
	if server == "" {
		for _, c2Name := range []string{SliverKind, BuiltInC2} {
			client, ok := clients[c2Name]
			if ok && client.IsReady() {
				return client, true
			}
		}
	}

	client, ok := clients[server]
	if ok {
		return client, true
	}
	return nil, false
}

// func onStartListener(mb bus.MessageBus, ctx context.Context, msg domain.Command, c2Client C2Client) (domain.Message, error) {
// 	cmd := msg.(domain.StartListener)
// 	c2Client.Execute(cmd)

// var wg sync.WaitGroup
// switch cmd.Server {
// case "":
// 	wg.Add(1)
// 	go func() {
// 		err := startListener(ctx, mb, cmd)
// 		if err != nil {
// 			slog.Error(err.Error())
// 		}
// 		// TODO handle disconnecting listener
// 		wg.Done()
// 	}()
// case "sliver":
// 	_, err := c2Client.Execute(cmd)
// 	if err != nil {
// 		slog.Error(err.Error())
// 	}
// }
// return startListener(ctx, mb, cmd.Port)
// 	return nil, nil
// }

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

	target := cmd.C2Channel.GetTarget()
	if target == nil {
		return "", "", fmt.Errorf("Could not exec command: No valid target selected!")
	}

	var targetName string
	var targetNs string
	// ensure target is actually a pod
	if pod, ok := target.(domain.Pod); ok {
		targetName = target.GetName()
		targetNs = pod.Namespace
	} else if workload, ok := target.(domain.Workload); ok {
		pods := workload.GetPods()
		if len(pods) > 0 {
			pod = pods[0]
			targetName = pod.Name
			targetNs = pod.Namespace
		} else {
			return "", "", fmt.Errorf("No target pod found in workload '%s'", target.GetName())
		}
	} else if e, ok := target.(domain.K8sEntity); ok {
		if e.Kind == "Pod" {
			targetName = e.Name
			targetNs = e.Namespace
		}
	}

	// TODO: handle case of multiple containers
	// TODO: handle mixture of command sources ... grounded template is currently on cmd.Cmd
	c := cmd.Cmd
	if c == "" {
		c = cmd.CmdVariants[0]
		// c = cmd.TTP.GetCommand("")
	}

	stdOut, stdErr, err := k8s.ExecInPod(ctx, client, targetName, targetNs, c, cmd.TTP.Args)
	return stdOut, stdErr, err
}

// Execute the code specified in the TTP.
// Note: this is highly insecure. Ensure you trust the code before allowing these TTPs.
func executeCode(snippet domain.CodeSnippet) error {
	var cmd *exec.Cmd
	if strings.ToLower(snippet.Lang) == "python" {
		var args = []string{"-c", snippet.Code}
		for k, v := range snippet.Parameters {
			args = append(args, "--"+k, v)
		}
		// TODO: maybe use `exec.CommandContext` to provide a timeout, so no reverse shells are possible?
		cmd = exec.Command("python3", args...)
	} else {
		return fmt.Errorf("'%s' not supported as execution language", snippet.Code)
	}
	var out bytes.Buffer
	cmd.Stdout = &out
	cmd.Stderr = &out
	err := cmd.Run()
	if err != nil {
		return fmt.Errorf("failed to execute python code: %s, error: %v", out.String(), err)
	}
	slog.Debug(out.String())
	return nil
}
