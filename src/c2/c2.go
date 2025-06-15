package c2

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"os"
	"os/exec"
	osexec "os/exec"

	"strconv"
	"strings"
	"sync"

	"github.com/Magier/Ran/core/bus"
	"github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
)

const BuiltInC2 = "Ran"

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

type C2Manager struct {
	bus     bus.MessageBus
	clients map[string]C2Client
}

func InitC2Manager(mb bus.MessageBus, sliverConfigPath string) C2Manager {
	c2Clients := map[string]C2Client{
		BuiltInC2: NewBuiltInServer(),
	}

	if sliverConfigPath != "" {
		if sliverClient, err := CreateSliverClient(sliverConfigPath); err != nil {
			slog.Warn(err.Error())
		} else {
			c2Clients[SliverKind] = sliverClient
		}
	}

	manager := C2Manager{
		bus:     mb,
		clients: c2Clients,
	}

	mb.Subscribe(domain.C2Connected{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		ev := msg.(domain.C2Connected)
		client, ok := manager.clients[ev.Name]
		var err error
		if ok {
			manager.clients[ev.Name] = client.SetReady(true)
		} else {
			err = fmt.Errorf("No suitable client found to update C2 state")
		}
		return nil, err
	})

	mb.Subscribe(domain.StartC2{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		cmd := msg.(domain.StartC2)
		return manager.StartC2Client(ctx, cmd.C2Name)
	})

	mb.Subscribe(domain.ExecTTP{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return manager.ExecuteTTP(ctx, msg, manager.clients)
	})

	return manager
}

func (c2 *C2Manager) Start(ctx context.Context) error {
	// listeners := make(map[string]net.Listener)
	// TODO start builtin C2 once an action demands it

	// TODO: send command after actually conncting to C2, with the right IP(s)
	c2EventStreams := make([]<-chan domain.Event, len(c2.clients))
	for _, client := range c2.clients {
		go connectToC2(ctx, c2.bus, client)
		c2EventStreams = append(c2EventStreams, client.GetEventStream())
	}

	for event := range fanIn[domain.Event](c2EventStreams...) {
		err := c2.bus.Publish(event)
		if err != nil {
			slog.Error("Failed to publish C2 event: " + err.Error())
		}
	}

	slog.Debug("Stopping C2 layer")
	return nil
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

func (c2 C2Manager) StartC2Client(ctx context.Context, c2Name string) (domain.Message, error) {
	client, ok := c2.clients[c2Name]
	if !ok {
		return nil, fmt.Errorf("'%s' is not a valid C2 server to connect to", c2Name)
	}
	go connectToC2(ctx, c2.bus, client)
	return nil, nil
}

func (c2 C2Manager) ExecuteTTP(ctx context.Context, msg domain.Message, c2Clients map[string]C2Client) (domain.Message, error) {
	exec := msg.(domain.ExecTTP)
	// check technique to execute CMD -> kubectl exec uses API
	// or shell listener?
	var err error
	results := make([]string, 0)

	switch cmd := exec.CommandMsg.(type) {
	case domain.StartListener:
		client, ok := selectClient(c2Clients, cmd)
		if ok {
			return client.Execute(cmd)
		}
		return nil, fmt.Errorf("No suitable client found to start listener")
	}

	if exec.Procedure.IsLocalCommand {
		results, err = execLocally(ctx, exec, exec.Procedure, c2Clients)
	} else if exec.C2Channel == nil {
		slog.Warn("No C2 channel defined - executing locally")
		results, err = execLocally(ctx, exec, exec.Procedure, c2Clients)
	} else {
		results, err = execRemotely(ctx, exec, exec.Procedure, c2Clients)
	}

	if err != nil {
		results = append(results, err.Error())
	}

	return domain.TTPExecuted{
		ID:        exec.ID,
		TTP:       exec.TTP,
		Args:      exec.Args,
		Procedure: exec.Procedure,
		Success:   wasExecSuccessful(results, err),
		Target:    exec.Target,
		Results:   results,
	}, nil
}

func wasExecSuccessful(results []string, err error) bool {
	if err != nil {
		return false
	}

	var stdout, stderr string
	if len(results) > 0 {
		stdout = results[0]
	}
	if len(results) > 1 {
		stderr = results[1]
	}

	if strings.Contains(strings.ToLower(stdout), "unauthorized") {
		return false
	}
	if strings.Contains(strings.ToLower(stderr), "not found") {
		return false
	}

	if strings.Contains(stderr, "Cannot exec") {
		return false
	}
	if strings.Contains(stderr, "exiting now") {
		return false
	}

	return true
}

func execLocally(ctx context.Context, exec domain.ExecTTP, procedure domain.Procedure, _ map[string]C2Client) ([]string, error) {
	var err error
	if procedure.Execute.Code != "" {
		err = executeCode(ctx, procedure.Command, procedure.Execute)
		if err != nil {
			slog.Warn(err.Error())
		}
	} else if procedure.Command != "" {
		if procedure.Key == "kubectl" && strings.Contains(procedure.Command, "exec") {
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

			podCfgJson, err := json.Marshal(podCfg)
			slog.Warn("‼️ Marshalled PodConfig JSON to str; please check it!!: ", string(podCfgJson))
			return []string{podName, ns, string(podCfgJson)}, err
		} else {
			fields := strings.Fields(procedure.Command)
			if len(fields) == 0 {
				return nil, fmt.Errorf("procedure.Command is empty")
			}
			cmd := osexec.Command(fields[0], fields[1:]...)
			// cmd.Stdin = strings.NewReader("some input")
			var out strings.Builder
			cmd.Stdout = &out
			err := cmd.Run()
			if err != nil {
				return nil, fmt.Errorf("Faild to  execute procedure '%s' locally", procedure.Command)
			}
			res := out.String()
			return []string{res}, nil
		}
	} else {
		return nil, errors.New("Can't Exec TTP: no channel defined and no code provided!")
	}
	return nil, err
}

// execRemotely uses a C2 channel to execute the command on the target system
func execRemotely(ctx context.Context, exec domain.ExecTTP, cmd domain.Procedure, c2Clients map[string]C2Client) ([]string, error) {
	target := exec.C2Channel.GetTarget()
	if target == nil {
		return nil, fmt.Errorf("Could not exec command: No valid target selected!")
	}

	var err error
	results := make([]string, 0)

	switch ch := exec.C2Channel.(type) {
	case domain.ImplantC2Channel:
		if c2, ok := c2Clients[exec.C2Channel.GetKind()]; ok {
			msg, err := c2.Execute(exec)
			if err != nil {
				results = append(results, err.Error())
			} else {
				results = append(results, msg.String())
			}
		}
	case domain.PodExecC2Channel:
		var stdout, stderr string
		stdout, stderr, err = execKubectl(ctx, cmd, target)
		results = []string{stdout, stderr}
		if err != nil {
			err = fmt.Errorf("%w: '%s'", err, stderr)
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

func execKubectl(ctx context.Context, cmd domain.Procedure, target domain.Entity) (string, string, error) {
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
func executeCode(ctx context.Context, bin string, snippet domain.CodeSnippet) error {
	var cmd *exec.Cmd
	if strings.ToLower(snippet.Lang) == "python" {
		var args = []string{"-c", snippet.Code}
		for k, v := range snippet.Parameters {
			args = append(args, "--"+strings.ToLower(k), v)
		}
		cmd = exec.CommandContext(ctx, bin, args...)
		if len(snippet.EnvVars) > 0 {
			cmd.Env = append(os.Environ(), snippet.EnvVars...)
		}
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
