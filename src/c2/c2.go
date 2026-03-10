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
	"github.com/Magier/Ran/mitre"
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
	bus          bus.MessageBus
	clients      map[string]C2Client
	clientsMutex sync.RWMutex
	newClientCh  chan C2Client
}

type ExecError struct {
	Message  string
	ExitCode int
}

func (e ExecError) Error() string {
	return fmt.Sprintf("(code %d): %s", e.ExitCode, e.Message)
}

func InitC2Manager(mb bus.MessageBus) *C2Manager {
	c2Clients := map[string]C2Client{
		BuiltInC2: NewBuiltInServer(),
	}

	manager := &C2Manager{
		bus:         mb,
		clients:     c2Clients,
		newClientCh: make(chan C2Client, 5), // Buffered to prevent blocking
	}

	mb.Subscribe(domain.C2Connected{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		ev := msg.(domain.C2Connected)
		manager.clientsMutex.Lock()
		client, ok := manager.clients[ev.Name]
		var err error
		if ok {
			manager.clients[ev.Name] = client.SetReady(true)
		} else {
			err = fmt.Errorf("No suitable client found to update C2 state")
		}
		manager.clientsMutex.Unlock()
		return nil, err
	})

	mb.Subscribe(domain.StartC2{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		cmd := msg.(domain.StartC2)
		return manager.StartC2Client(ctx, cmd.C2Name)
	})

	mb.Subscribe(domain.ExecTTP{}, func(ctx context.Context, cmd domain.Message) (domain.Message, error) {
		msg, err := manager.ExecuteTTP(ctx, cmd)
		// ensure all errors are treated as failed TTP executions, to surface the underlying error
		if err != nil {
			ev := cmd.(domain.ExecTTP)
			msg = TTPExecuted{
				ID:      ev.ID,
				Success: false,
				Results: []string{err.Error()},
			}
		}
		return msg, nil
	})

	return manager
}

func (c2 *C2Manager) Start(ctx context.Context) error {
	// Connect to all initial clients and collect their event streams
	c2.clientsMutex.RLock()
	initialClients := make([]C2Client, 0, len(c2.clients))
	for _, client := range c2.clients {
		initialClients = append(initialClients, client)
	}
	c2.clientsMutex.RUnlock()

	for _, client := range initialClients {
		err := client.Connect(ctx)
		if err != nil {
			slog.Error("Failed to connect to C2 server", "error", err)
		}
	}

	// Create a dynamic event multiplexer that can handle new clients
	eventChan := make(chan domain.Event, 100)
	wg := sync.WaitGroup{}

	// Track active event stream goroutines
	activeStreams := make(map[string]context.CancelFunc)
	streamsMutex := sync.Mutex{}

	// Helper to start monitoring a client's event stream
	startEventStream := func(client C2Client) {
		stream := client.GetEventStream()
		if stream == nil {
			return
		}

		streamsMutex.Lock()
		// Check if already monitoring this client
		if _, exists := activeStreams[client.GetName()]; exists {
			streamsMutex.Unlock()
			return
		}

		// Create a cancellable context for this stream
		streamCtx, streamCancel := context.WithCancel(ctx)
		activeStreams[client.GetName()] = streamCancel
		streamsMutex.Unlock()

		wg.Add(1)
		go func(clientName string) {
			defer wg.Done()
			defer func() {
				streamsMutex.Lock()
				delete(activeStreams, clientName)
				streamsMutex.Unlock()
			}()

			for {
				select {
				case <-streamCtx.Done():
					return
				case event, ok := <-stream:
					if !ok {
						return
					}
					select {
					case eventChan <- event:
					case <-streamCtx.Done():
						return
					}
				}
			}
		}(client.GetName())
	}

	// Start monitoring initial clients
	for _, client := range initialClients {
		startEventStream(client)
	}

	// Main event loop: handle events and new clients
	wg.Add(1)
	go func() {
		defer wg.Done()
		for {
			select {
			case <-ctx.Done():
				slog.Debug("C2 Manager context cancelled")
				return

			case client, ok := <-c2.newClientCh:
				if !ok {
					slog.Debug("New client channel closed")
					return
				}
				slog.Info("Adding event stream for new C2 client", "client", client.GetName())
				startEventStream(client)

			case event, ok := <-eventChan:
				if !ok {
					slog.Debug("Event channel closed")
					return
				}
				if err := c2.bus.Publish(event); err != nil {
					slog.Error("Failed to publish C2 event", "error", err)
				}
			}
		}
	}()

	// Wait for context cancellation
	<-ctx.Done()

	// Cancel all active streams
	streamsMutex.Lock()
	for _, cancel := range activeStreams {
		cancel()
	}
	streamsMutex.Unlock()

	// Close channels and wait for goroutines
	close(c2.newClientCh)
	close(eventChan)
	wg.Wait()

	slog.Debug("Stopping C2 layer")
	return nil
}

func (c2 *C2Manager) StartC2Client(ctx context.Context, c2Name string) (domain.Message, error) {
	c2.clientsMutex.RLock()
	client, ok := c2.clients[c2Name]
	c2.clientsMutex.RUnlock()

	if !ok {
		return nil, fmt.Errorf("'%s' is not a valid C2 server to connect to", c2Name)
	}

	err := client.Connect(ctx)
	if err != nil {
		return nil, err
	}

	// Notify the Start function about the new client so it can monitor its event stream
	select {
	case c2.newClientCh <- client:
		slog.Info("Notified C2 Manager about new client", "client", c2Name)
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
		slog.Warn("Could not notify about new client (channel full)", "client", c2Name)
	}

	return nil, nil
}

// AddClient adds a new C2 client at runtime and integrates it with the event system
func (c2 *C2Manager) AddClient(ctx context.Context, name string, client C2Client) error {
	c2.clientsMutex.Lock()
	if _, exists := c2.clients[name]; exists {
		c2.clientsMutex.Unlock()
		return fmt.Errorf("client '%s' already exists", name)
	}
	c2.clients[name] = client
	c2.clientsMutex.Unlock()

	// Connect the client
	if err := client.Connect(ctx); err != nil {
		// Remove the client if connection fails
		c2.clientsMutex.Lock()
		delete(c2.clients, name)
		c2.clientsMutex.Unlock()
		return fmt.Errorf("failed to connect client '%s': %w", name, err)
	}

	// Notify the Start function about the new client so it can monitor its event stream
	select {
	case c2.newClientCh <- client:
		slog.Info("Added and connected new C2 client", "client", name)
	case <-ctx.Done():
		return ctx.Err()
	default:
		slog.Warn("Could not notify about new client (channel full), events may be delayed", "client", name)
	}

	return nil
}

func (c2 *C2Manager) ExecuteTTP(ctx context.Context, msg domain.Message) (domain.Message, error) {
	exec := msg.(domain.ExecTTP)
	// check technique to execute CMD -> kubectl exec uses API
	// or shell listener?
	var err error
	var execTarget domain.System
	var resMsg domain.Message
	results := make([]string, 0)

	if exec.CommandMsg != nil {
		switch cmd := exec.CommandMsg.(type) {
		case domain.StartListener:
			c2.clientsMutex.RLock()
			client, ok := selectClient(c2.clients, cmd.Server, true)
			c2.clientsMutex.RUnlock()
			if ok {
				resMsg, err = client.Execute(cmd)
			} else {
				err = fmt.Errorf("No suitable client found to start listener")
			}
		case domain.StopListener:
			c2.clientsMutex.RLock()
			client, ok := selectClient(c2.clients, cmd.Server, true)
			c2.clientsMutex.RUnlock()
			if ok {
				resMsg, err = client.Execute(cmd)
			} else {
				err = fmt.Errorf("No suitable client found to stop listener")
			}
		}

		if resMsg != nil {
			slog.Info("Executed command: " + resMsg.String())
			_ = c2.bus.Publish(resMsg)
		}
	} else {
		c2.clientsMutex.RLock()
		c2Client, hasC2Client := selectClient(c2.clients, exec.Procedure.Tool, false)
		c2.clientsMutex.RUnlock()

		if strings.HasPrefix(exec.Procedure.Command, "c2") {
			switch exec.Procedure.Command {
			case "c2.connect":
				if exec.Procedure.Tool == SliverKind {
					cfgPath := exec.Args["CONFIG_PATH"]
					if sliverClient, err := CreateSliverClient(cfgPath); err == nil {
						c2.clientsMutex.Lock()
						c2.clients[SliverKind] = &sliverClient
						c2.clientsMutex.Unlock()
					} else {
						return nil, fmt.Errorf("Failed to create Sliver client: %w", err)
					}
				}
				var responseMsg domain.Message
				responseMsg, err = c2.StartC2Client(ctx, exec.Procedure.Tool)
				if err == nil && responseMsg != nil {
					if e := c2.bus.Publish(responseMsg); e != nil {
						slog.Error("Failed to publish C2 client start message: " + e.Error())
					}
				}
			case "c2.close":
				if hasC2Client {
					c2Client.Shutdown()
					c2.clientsMutex.Lock()
					delete(c2.clients, exec.Procedure.Tool)
					c2.clientsMutex.Unlock()
				} else {
					slog.Warn(fmt.Sprintf("No client found for C2 '%s' to close", exec.Procedure.Tool))
				}
			}
		} else if strings.HasPrefix(exec.Procedure.Command, "setTarget") {
			if createSession, ok := exec.Args["Session"]; ok && createSession == "true" {
				c2.clientsMutex.RLock()
				ranC2 := c2.clients[BuiltInC2].(*BuiltInC2Server)
				c2.clientsMutex.RUnlock()
				// ranC2.SetTarget(exec.C2Channel.GetTarget())
				err := ranC2.EstablishPodExecShell(ctx, exec.Args["Namespace"], exec.Args["PodName"])
				if err != nil {
					slog.Error("Failed to set target for session: " + err.Error())
					results = append(results, "Failed to set target for session: "+err.Error())
				} else {
					results = append(results, "ok")
				}
			} else {
				// this is a special case, as it does not execute a command, but sets the target for the next commands on the same channel
				results = append(results, "ok")
			}
		} else if hasC2Client && c2Client.IsReady() {
			msg, err := c2Client.Execute(exec)
			if err != nil {
				results = append(results, err.Error())
			} else if msg != nil {
				results = append(results, msg.String())
			} else {
				// ensure some signal of successful execution is returned
				// which is relevant for later business logic
				results = append(results, "ok")
			}
		} else if exec.Procedure.IsLocalCommand {
			results, err = execLocally(ctx, exec, exec.Procedure, c2.clients)
		} else if exec.C2Channel == nil {
			slog.Warn("No C2 channel defined - executing locally")
			results, err = execLocally(ctx, exec, exec.Procedure, c2.clients)
		} else {
			results, err = execRemotely(ctx, exec, exec.Procedure, c2.clients)
			var ok bool
			execTarget, ok = exec.C2Channel.GetFinalTarget().(domain.System)
			if !ok {
				slog.Warn(fmt.Sprintf("Could not get on which system TTP was executed: %T", exec.C2Channel.GetTarget()))
			}

			// Kubelet exec channels return a JSON envelope from the websocket tool;
			// unwrap it to extract the actual command output and detect failures.
			if hasKubeletExecSink(exec.C2Channel) && len(results) > 0 && err == nil {
				unwrapped, parseErr := parseJSONResponse(results[0])
				results[0] = unwrapped
				if parseErr != nil {
					err = parseErr
				}
			}

			// TODO: properly fix this dirty hack:
			if exec.Procedure.Key == "grep" {
				err = nil
			}
		}
	}

	var exitCode int
	var failReason string
	if err != nil {
		failReason = err.Error()
		results = append(results, err.Error())
		var execErr ExecError
		if errors.As(err, &execErr) {
			exitCode = execErr.ExitCode
			failReason = execErr.Message
		}
	}

	// Temporary work around to not return TTPExecuted, when it's an async execution
	// and neither positive nor negative results are in
	// TODO: this needs to be properly synced with the toast in the UI
	// the attack step tracing works with a any other TTPExecuted call
	if len(results) == 0 && err == nil {
		return nil, nil
	}

	if execTarget == nil && !exec.Procedure.IsLocalCommand {
		slog.Warn("Could not determine executedOn for executed TTP")
	}

	return TTPExecuted{
		ID:              exec.ID,
		Success:         wasExecSuccessful(results, err),
		ExecutedOn:      execTarget,
		ExecutedLocally: exec.Procedure.IsLocalCommand,
		Results:         results,
		FailReason:      failReason,
		ExitCode:        exitCode,
	}, nil
	// return domain.TTPExecuted{
	// 	ID:         exec.ID,
	// 	TTP:        exec.TTP,
	// 	Args:       exec.Args,
	// 	Procedure:  exec.Procedure,
	// 	Success:    wasExecSuccessful(results, err),
	// 	Target:     exec.Target,
	// 	ExecutedOn: execTarget,
	// 	Results:    results,
	// 	WasCleanup: exec.IsCleanup,
	// }, nil
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

	if len(stdout) == 0 && len(stderr) > 0 {
		return false
	}

	return true
}

// hasKubeletExecSink checks if the C2 channel chain contains a KubeletExecSink,
// indicating the response will be a JSON envelope from a websocket tool.
func hasKubeletExecSink(ch domain.C2Channel) bool {
	for c := ch; c != nil; c = c.GetNextChannel() {
		if _, ok := c.(*domain.KubeletExecSink); ok {
			return true
		}
	}
	return false
}

// jsonExecResponse represents the JSON output from websocket-based tools (ran-ws, ws.py).
type jsonExecResponse struct {
	Result  string `json:"result"`
	Status  string `json:"status"`
	Message string `json:"message"`
}

// parseJSONResponse parses a JSON-formatted tool response and extracts the result.
// Returns the unwrapped result string and an error if the tool failed.
// If stdout is not valid JSON, it means the binary execution itself failed.
func parseJSONResponse(stdout string) (string, error) {
	stdout = strings.TrimSpace(stdout)
	if stdout == "" {
		return "", fmt.Errorf("empty response from tool (binary may have failed)")
	}
	var resp jsonExecResponse
	if err := json.Unmarshal([]byte(stdout), &resp); err != nil {
		return stdout, fmt.Errorf("tool output is not valid JSON (binary may have failed): %s", stdout)
	}
	if resp.Status == "Failure" {
		return resp.Result, fmt.Errorf("command failed: %s", resp.Message)
	}
	return resp.Result, nil
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
			slog.Warn("‼️ Marshalled PodConfig JSON to str; please check it!!: " + string(podCfgJson))
			return []string{podName, ns, string(podCfgJson)}, err
		} else {
			fields := strings.Fields(procedure.Command)
			if len(fields) == 0 {
				return nil, fmt.Errorf("procedure.Command is empty")
			}
			cmd := osexec.Command(fields[0], fields[1:]...)
			// cmd.Stdin = strings.NewReader("some input")
			var stdout, stderr strings.Builder
			cmd.Stdout = &stdout
			cmd.Stderr = &stderr
			err := cmd.Run()
			if err != nil {
				return nil, fmt.Errorf("Failed to execute procedure '%s' locally: %s", procedure.Command, stderr.String())
			}
			return []string{stdout.String(), stderr.String()}, nil
		}
	} else if exec.TTP.Tactic == mitre.InitialAccess {
		// initial access TTPs should have a command to set the target
		return []string{"ok"}, nil
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
	case *domain.ImplantC2Channel:
		if c2, ok := c2Clients[exec.C2Channel.GetKind()]; ok {
			msg, err := c2.Execute(exec)
			if err != nil {
				results = append(results, err.Error())
			} else {
				results = append(results, msg.String())
			}
		}
	case *domain.PodExecC2Channel:
		var stdout, stderr string

		if ch.IsInteractive {
			slog.Debug("Establishing interactive shell for channel  is not yet implemented!")
		} else {
			stdout, stderr, err = execKubectl(ctx, cmd, target)
			results = []string{stdout, stderr}
			if err != nil {
				if execErr, ok := err.(k8s.ExecError); ok {
					err = ExecError{
						Message:  execErr.Error(),
						ExitCode: execErr.Code,
					}
				} else {
					err = fmt.Errorf("%w: '%s'", err, stderr)
				}
			}
		}
	default:
		slog.Warn(fmt.Sprintf("Can't Exec TTP: unclear how to handle channel %v", ch))
	}
	return results, err
}

func selectClient(clients map[string]C2Client, name string, selectAny bool) (C2Client, bool) {
	// no server defined means the C2 will choose the best option
	if selectAny {
		for _, c2Name := range []string{SliverKind, BuiltInC2} {
			client, ok := clients[c2Name]
			if ok && client.IsReady() {
				return client, true
			}
		}
	}

	client, ok := clients[name]
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

func getPodNameAndNamespace(target domain.Entity) (string, string, error) {
	// TODO: handle case of multiple containers
	var podName string
	var targetNs string
	// ensure target is actually a pod
	if pod, ok := target.(domain.Pod); ok {
		podName = target.GetName()
		targetNs = pod.Namespace
	} else if workload, ok := target.(domain.Workload); ok {
		pods := workload.GetPods()
		if len(pods) > 0 {
			pod = pods[0]
			podName = pod.Name
			targetNs = pod.Namespace
		} else {
			return "", "", fmt.Errorf("No target pod found in workload '%s'", target.GetName())
		}
	} else if e, ok := target.(domain.K8sEntity); ok {
		if e.Kind == "Pod" {
			podName = e.Name
			targetNs = e.Namespace
		}
	}
	return targetNs, podName, nil
}

func execKubectl(ctx context.Context, cmd domain.Procedure, target domain.Entity) (string, string, error) {
	client, err := k8s.NewK8sClient("")
	if err != nil {
		return "", "", err
	}

	targetNs, podName, err := getPodNameAndNamespace(target)
	if err != nil {
		return "", "", err
	}
	stdOut, stdErr, err := k8s.ExecInPod(ctx, client, podName, targetNs, cmd.Command)
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
