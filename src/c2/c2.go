package c2

import (
	"context"
	"fmt"
	"log/slog"
	"net"
	"sync"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
	k8s "github.com/Magier/Ran/internal/k8sclient"
)

type C2Client interface {
	Connect(context.Context, bus.MessageBus) error
	Execute(domain.Command) (domain.Message, error)
	GetServerIp() net.IP
}

type C2Started struct {
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

func (c SessionStarted) String() string {
	return "Session started: " + c.Session.Id
}

type SessionClosed struct {
	Session Session
}

func (c SessionClosed) String() string {
	return "Session closed: " + c.Session.Id
}

func StartC2(ctx context.Context, mb bus.MessageBus) {
	// listeners := make(map[string]net.Listener)
	// TODO start builtin C2 once an action demands it
	c2Clients := map[string]C2Client{
		"":       NewBuiltInServer(mb),
		"sliver": CreateSliverClient("../sliver_cfg.json"),
	}

	mb.Subscribe(domain.StartListener{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		client, ok := selectClient(c2Clients, event)
		if ok {
			return onStartListener(mb, ctx, event, client)
		}
		return nil, fmt.Errorf("No suitable client found to start listener")
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
		slog.Error("C2", "can't publish c2 started event:", err.Error())
	}

	var wg sync.WaitGroup
	for _, client := range c2Clients {
		go func() {
			wg.Add(1)
			defer wg.Done()
			err := client.Connect(ctx, mb)
			if err != nil {
				slog.Error(err.Error())
			}
		}()
	}
	wg.Wait()
}

func selectClient(clients map[string]C2Client, event domain.Event) (C2Client, bool) {
	var server string
	switch cmd := event.(type) {
	case domain.StartListener:
		server = cmd.Server
	case domain.StopListener:
		server = cmd.Server
	}

	client, ok := clients[server]
	if ok {
		return client, true
	}
	return nil, false
}

func onStartListener(mb bus.MessageBus, ctx context.Context, event domain.Event, c2Client C2Client) (domain.Message, error) {
	cmd := event.(domain.StartListener)
	c2Client.Execute(cmd)

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
	return nil, nil
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
