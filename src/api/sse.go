package api

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"net/http"
	"sync"
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

	data, err := json.Marshal(v)
	if err != nil {
		slog.Error("Failed to marshal SSE message", "error", err)
		return err
	}

	client.mu.Lock()
	defer client.mu.Unlock()

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

	// Keep connection alive until client disconnects
	<-r.Context().Done()
	slog.Info("SSE client disconnected")
}
