package api

import (
	"fmt"
	"log/slog"
	"net/http"
	"sync"
	"time"
)

const (
	marshalMaxRetries = 3
	marshalRetryDelay = 10 * time.Millisecond
)

type SSEClient struct {
	w       http.ResponseWriter
	flusher http.Flusher
	mu      sync.Mutex
	done    chan struct{}
}

func NewSSEClient(w http.ResponseWriter) (*SSEClient, error) {
	flusher, ok := w.(http.Flusher)
	if !ok {
		return nil, fmt.Errorf("streaming not supported")
	}

	return &SSEClient{
		w:       w,
		flusher: flusher,
		done:    make(chan struct{}),
	}, nil
}

func (client *SSEClient) Close() error {
	select {
	case <-client.done:
		// Already closed
	default:
		close(client.done)
	}
	return nil
}

func (client *SSEClient) sendJSON(name string, v interface{}) error {
	select {
	case <-client.done:
		return fmt.Errorf("client disconnected")
	default:
	}

	if _, ok := v.(WSResponse); !ok {
		switch err := v.(type) {
		case error:
			v = WSResponse{Type: name, Error: err.Error()}
		default:
			v = WSResponse{Type: name, Data: v}
		}
	}

	client.mu.Lock()
	defer client.mu.Unlock()

	// safeJSONMarshal creates a snapshot to prevent concurrent map access panics.
	// Retry on failure since the root cause is usually a transient race window
	// between snapshotValue and a concurrent map write in another goroutine.
	var data []byte
	var err error
	for attempt := 1; attempt <= marshalMaxRetries; attempt++ {
		data, err = safeJSONMarshal(v)
		if err == nil {
			break
		}
		slog.Warn("Failed to marshal SSE message, retrying", "event", name, "attempt", attempt, "error", err)
		time.Sleep(marshalRetryDelay)
	}
	if err != nil {
		slog.Error("Failed to marshal SSE message after retries", "event", name, "attempts", marshalMaxRetries, "error", err)
		return err
	}

	// SSE format: event: <name>\ndata: <json>\n\n
	_, err = fmt.Fprintf(client.w, "event: %s\ndata: %s\n\n", name, string(data))
	if err != nil {
		return err
	}

	client.flusher.Flush()
	return nil
}

func (a *API) handleSSE(w http.ResponseWriter, r *http.Request) {
	// Set SSE headers
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	w.Header().Set("Access-Control-Allow-Origin", "*")

	client, err := NewSSEClient(w)
	if err != nil {
		http.Error(w, "SSE not supported", http.StatusInternalServerError)
		return
	}

	a.clientsMu.Lock()
	a.clients[client] = true
	a.clientsMu.Unlock()

	defer func() {
		a.clientsMu.Lock()
		delete(a.clients, client)
		a.clientsMu.Unlock()
		client.Close()
	}()

	// Send initial armory data
	if err := client.sendJSON("armory-loaded", a.ran.Armory.GetTTPs()); err != nil {
		slog.Error("Failed to send armory via SSE", "error", err)
		return
	}

	slog.Info("SSE client connected")

	// Send periodic keepalive pings to prevent idle timeout
	ticker := time.NewTicker(15 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-ticker.C:
			client.mu.Lock()
			_, err := fmt.Fprintf(client.w, ": ping\n\n")
			if err != nil {
				client.mu.Unlock()
				slog.Info("SSE client disconnected (write error)")
				return
			}
			client.flusher.Flush()
			client.mu.Unlock()
		case <-r.Context().Done():
			slog.Info("SSE client disconnected")
			return
		}
	}
}
