package c2

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"os/exec"
	"strconv"
	"strings"
	"sync"

	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
	k8s "github.com/Magier/Ran/k8sclient"
)

const BuiltInC2 = "builtin"

type C2Client interface {
	Connect(context.Context) error
	Execute(domain.Command) (domain.Message, error)
	GetServerIp() net.IP
	GetName() string
	IsReady() bool
	SetReady(state bool) C2Client
	GetEventStream() <-chan domain.Event
	Shutdown()
}

func StartC2(ctx context.Context, mb bus.MessageBus) {
	// listeners := make(map[string]net.Listener)
	// TODO start builtin C2 once an action demands it
	c2Clients := map[string]C2Client{
		BuiltInC2:  NewBuiltInServer(),
		SliverKind: CreateSliverClient("../sliver_cfg.json"),
	}

	mb.Subscribe(domain.C2Connected{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		ev := msg.(domain.C2Connected)
		client, ok := c2Clients[ev.Name]
		var err error
		if ok {
			c2Clients[ev.Name] = client.SetReady(true)
		} else {
			err = fmt.Errorf("No suitable client found to update C2 state")
		}
		return nil, err
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

	mb.Subscribe(domain.ExecTTP{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return executeTTP(ctx, msg, c2Clients)
	})

	// TODO: send command after actually conncting to C2, with the right IP(s)
	c2EventStreams := make([]<-chan domain.Event, len(c2Clients))
	for _, client := range c2Clients {
		go connectToC2(ctx, mb, client)
		c2EventStreams = append(c2EventStreams, client.GetEventStream())
	}

	for event := range fanIn[domain.Event](c2EventStreams...) {
		err := mb.Publish(event)
		if err != nil {
			slog.Error("Failed to publish C2 event: " + err.Error())
		}
	}

	slog.Debug("Stopping C2 layer")
}

func fanIn[T domain.Event](channels ...<-chan T) chan T {
	wg := sync.WaitGroup{}
	wg.Add(len(channels))
	output := make(chan T)
	for _, c := range channels {
		go func(channel <-chan T) {
			defer wg.Done()
			for i := range channel {
				output <- i
				// select {
				// case output <- i:
				// 	return
				// }
			}
		}(c)
	}
	go func() {
		wg.Wait()
		close(output)
	}()
	return output
}

func connectToC2(ctx context.Context, mb bus.MessageBus, c2client C2Client) {
	// successful C2Connect event is sent by the client itself
	err := c2client.Connect(ctx)
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

func executeTTP(ctx context.Context, msg domain.Message, c2Clients map[string]C2Client) (domain.Message, error) {
	exec := msg.(domain.ExecTTP)
	// check technique to execute CMD -> kubectl exec uses API
	// or shell listener?
	var err error
	results := make([]any, 0)

	switch cmd := exec.CommandMsg.(type) {
	case domain.StartListener:
		client, ok := selectClient(c2Clients, cmd)
		if ok {
			return client.Execute(cmd)
		}
		return nil, fmt.Errorf("No suitable client found to start listener")
	}

	if exec.Variant.IsLocalCommand || exec.C2Channel == nil {
		results, err = execLocally(ctx, exec, exec.Variant, c2Clients)
	} else {
		results, err = execRemotely(ctx, exec, exec.Variant, c2Clients)
	}

	if err != nil {
		return domain.TTPFailed{
			ID:     exec.ID,
			TTP:    exec.TTP,
			Reason: err.Error(),
		}, nil
	}
	return domain.TTPExecuted{
		ID:      exec.ID,
		TTP:     exec.TTP,
		Target:  exec.Target,
		Results: results,
	}, nil
}

func execLocally(ctx context.Context, exec domain.ExecTTP, cmd domain.CmdVariant, _ map[string]C2Client) ([]any, error) {
	var err error
	if exec.TTP.Execute.Code != "" {
		err = executeCode(exec.TTP.Execute)
		if err != nil {
			slog.Warn(err.Error())
		}
	} else if cmd.Command != "" {
		if cmd.Key == "kubectl" {
			// TODO use the custom k8sclient to execute this -> generalize kubectl exec
			client, err := k8s.NewK8sClient("")
			if err != nil {
				return nil, err
			}

			podName := exec.Args["Name"]
			ns := exec.Args["Namespace"]
			image := exec.Args["Image"]
			cmd := exec.Args["Command"]
			nodeName := exec.Args["NodeName"]

			checkFlag := func(key string) (bool, error) {
				valStr, ok := exec.Args[key]
				if ok {
					val, err := strconv.ParseBool(valStr)
					if err != nil {
						return false, fmt.Errorf("invalid %s value '%s': %w", key, key, err)
					}
					return val, nil
				}
				return false, fmt.Errorf("'%s' is not a valid argument", key)
			}

			hostNetwork, _ := checkFlag("HostNetwork")
			hostIPC, _ := checkFlag("HostIPC")
			hostPID, _ := checkFlag("HostPID")
			privileged, _ := checkFlag("Privileged")
			podCfg := domain.PodConfig{
				Image:       image,
				Command:     cmd,
				HostIPC:     hostIPC,
				HostPID:     hostPID,
				HostNetwork: hostNetwork,
				Privileged:  privileged,
				NodeName:    nodeName,
			}
			status, err := k8s.DeployPod(ctx, client, podName, ns, podCfg)
			var _ = status
			return []any{podName, ns, podCfg}, err
		} else {
			slog.Warn(fmt.Sprintf("Unclear how to locally execute variant '%s'", cmd.Command))
		}
	} else {
		slog.Warn("Can't Exec TTP: no channel defined and no code provided!")
	}
	return nil, err
}

// execRemotely uses a C2 channel to execute the command on the target system
func execRemotely(ctx context.Context, exec domain.ExecTTP, cmd domain.CmdVariant, c2Clients map[string]C2Client) ([]any, error) {
	target := exec.C2Channel.GetTarget()
	if target == nil {
		return nil, fmt.Errorf("Could not exec command: No valid target selected!")
	}

	var err error
	results := make([]any, 0)

	switch ch := exec.C2Channel.(type) {
	case domain.ImplantC2Channel:
		if c2, ok := c2Clients[exec.C2Channel.GetKind()]; ok {
			var res any
			res, err = c2.Execute(exec)
			results = append(results, res)
		}
	case domain.PodExecC2Channel:
		var stdout, stderr string
		stdout, stderr, err = execKubectl(ctx, cmd, target)
		if err != nil {
			err = fmt.Errorf("%w: '%s'", err, stderr)
		} else if stdout == "" && strings.Contains(stderr, ": not found") {
			err = errors.New(stderr)
			// msg, err := cmd.TTP.HandleResult(cmd.Target, stdout, stderr)
			// if err != nil {
			// 	msg = domain.TTPFailed{ID: cmd.TTP.ID, TTP: cmd.TTP, Reason: err.Error()}
			// } else if msg == nil { // no handler -> try default handler for ExecTTP
			// 	return handleExecTTPResult(cmd, stdout, stderr)
			// }
			// return msg, err
		} else {
			results = []any{stdout, stderr}
		}
	default:
		slog.Warn(fmt.Sprintf("Can't Exec TTP: unclear how to handle channel %v", ch))
	}
	return results, err
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

func execKubectl(ctx context.Context, cmd domain.CmdVariant, target domain.Entity) (string, string, error) {
	client, err := k8s.NewK8sClient("")
	if err != nil {
		return "", "", err
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
	stdOut, stdErr, err := k8s.ExecInPod(ctx, client, targetName, targetNs, cmd.Command)
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
