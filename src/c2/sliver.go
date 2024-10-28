package c2

import (
	"context"
	"fmt"
	"io"
	"log"
	"log/slog"
	"net"
	"strings"

	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
	"github.com/bishopfox/sliver/client/assets"
	consts "github.com/bishopfox/sliver/client/constants"
	"github.com/bishopfox/sliver/client/transport"
	"github.com/bishopfox/sliver/protobuf/clientpb"
	"github.com/bishopfox/sliver/protobuf/commonpb"
	"github.com/bishopfox/sliver/protobuf/rpcpb"
)

func makeRequest(session *clientpb.Session) *commonpb.Request {
	if session == nil {
		return nil
	}
	timeout := int64(60)
	return &commonpb.Request{
		SessionID: session.ID,
		Timeout:   timeout,
	}
}

type SliverClient struct {
	rpc rpcpb.SliverRPCClient
}

func ConnectToSliverServer(ctx context.Context, bus bus.MessageBus, configPath string, cmdChannel <-chan domain.Command) {
	// load the client configuration from the filesystem
	config, err := assets.ReadConfig(configPath)
	if err != nil {
		log.Fatal(err)
	}
	// connect to the server
	rpc, ln, err := transport.MTLSConnect(config)
	if err != nil {
		slog.Error(err.Error())
	}

	err = bus.Publish(domain.ConnectedToExternalC2Server{
		Name: "Sliver",
		Ip:   config.LHost,
		Type: "Sliver",
	})
	if err != nil {
		slog.Warn("Couldn't send 'C2 Connected' event: ", "", err.Error())
		return
	}
	defer ln.Close()

	reportOpenListeners(rpc, bus, config.LHost, config.LPort)

	// Open the event stream to be able to collect all events sent by  the server
	eventStream, err := rpc.Events(context.Background(), &commonpb.Empty{})
	if err != nil {
		slog.Error(err.Error())
	}

	events := make(chan *clientpb.Event)
	go func(eventStream rpcpb.SliverRPC_EventsClient) {
		for {
			event, err := eventStream.Recv()
			if err == io.EOF || event == nil {
				return
			}

			events <- event
		}
	}(eventStream)

	for {
		select {
		case cmd, ok := <-cmdChannel:
			if !ok {
				cmdChannel = nil
			}
			err = executeCommand(cmd)
			if err != nil {
				slog.Error("Sliver C2", "Could not send cmd", err.Error())
			}

		case event := <-events:
			go handleSliverEvent(bus, event)
		}
	}
}

func handleSliverEvent(bus bus.MessageBus, event *clientpb.Event) (domain.Message, error) {
	var resultingMessage domain.Message
	// Trigger event based on type
	switch event.EventType {
	// a new session just came in
	case consts.SessionOpenedEvent:
		resultingMessage = SessionStarted{Session: parseSession(event.Session)}
	case consts.SessionClosedEvent:
		resultingMessage = SessionClosed{Session: parseSession(event.Session)}

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
	}

	err := bus.Publish(resultingMessage)
	if err != nil {
		slog.Error("Error publishing session started event:", err.Error(), "")
		return nil, err
	}
	return nil, nil
}
}

func parseSession(session *clientpb.Session) Session {
	return Session{
		Id:       session.ID,
		Hostname: session.Hostname,
		Os:       session.OS,
		User:     session.Username,
	}
}

func reportOpenListeners(rpc rpcpb.SliverRPCClient, bus bus.MessageBus, serverIp string, clientPort int) {
	jobs, err := rpc.GetJobs(context.Background(), &commonpb.Empty{})
	if err != nil {
		slog.Error(err.Error())
	}

	// resolve the local IP to the 'external' one, so the compromised systems can reach it
	var ip net.IP
	if serverIp == "0.0.0.0" || serverIp == "localhost" {
		ip = GetOutboundIP()
	} else {
		ip = net.ParseIP(serverIp)
	}
	for _, job := range jobs.Active {
		// no need to leak infrastructure details
		// so ignore the listener for new (multiplayer) clients
		if job.Port == uint32(clientPort) {
			continue
		}

		err = bus.Publish(ListenerReady{
			Name:     fmt.Sprintf("sliver_%s", job.Name),
			IP:       ip,
			Port:     uint(job.Port),
			Protocol: domain.Protocol(strings.ToUpper(job.Protocol)),
		})
		if err != nil {
			slog.Error("Error publishing listener event: " + err.Error())
		}
	}
}

func StartSliverListener(ev domain.StartListener) error {
	// TODO: get client
	// start listener
	return nil
}
