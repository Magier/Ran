package api

import (
	"encoding/json"
	"log"
	"log/slog"
	"net/http"
	"sync"

	"github.com/Magier/Ran/domain"
	"github.com/gorilla/websocket"
)

type WSRequest struct {
	Type   string          `json:"type"`
	Params json.RawMessage `json:"params,omitempty"`
}

type WSResponse struct {
	Type  string      `json:"type"`
	Data  interface{} `json:"data,omitempty"`
	Error string      `json:"error,omitempty"`
}

var upgrader = websocket.Upgrader{
	ReadBufferSize:  1024,
	WriteBufferSize: 1024,
	CheckOrigin: func(r *http.Request) bool {
		return true // Allow all origins for development
	},
}

type WSClient struct {
	conn *websocket.Conn
	mu   sync.Mutex
}

func (client *WSClient) Close() error {
	return client.conn.Close()
}

func (client *WSClient) sendJSON(name string, v interface{}) error {
	if _, ok := v.(WSResponse); !ok {
		switch err := v.(type) {
		case error:
			v = WSResponse{Type: name, Error: err.Error()}
		default:
			v = WSResponse{Type: name, Data: v}
		}
	}

	data, err := safeJSONMarshal(v)
	if err != nil {
		slog.Error("Failed to marshal response", "error", err)
		return err
	}

	client.mu.Lock()
	defer client.mu.Unlock()
	if err := client.conn.WriteMessage(websocket.TextMessage, data); err != nil {
		slog.Error("Failed to write message", "error", err)
		return err
	}
	return nil
}

func (a *API) handleWebSocket(w http.ResponseWriter, r *http.Request) {
	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		slog.Error("WebSocket upgrade failed", "error", err)
		return
	}
	defer conn.Close()

	client := &WSClient{conn: conn}

	a.clientsMu.Lock()
	a.clients[client] = true
	a.clientsMu.Unlock()

	defer func() {
		if r := recover(); r != nil {
			log.Fatal("Recovered from panic in socket handler", r)
		}
		a.clientsMu.Lock()
		delete(a.clients, client)
		a.clientsMu.Unlock()
	}()

	// conn.WriteMessage(websocket.TextMessage, []byte("{\"type\": \"test\", \"data\": \"Message received\"}"))
	// a.ran.ReplayEvents()

	// send Armory on connect
	if err := client.sendJSON("armory-loaded", a.ran.Armory.GetTTPs()); err != nil {
		slog.Error("Failed to send armory on connect", "error", err)
		return
	}

	for {
		_, message, err := conn.ReadMessage()
		if err != nil {
			if websocket.IsUnexpectedCloseError(err, websocket.CloseGoingAway, websocket.CloseAbnormalClosure) {
				slog.Error("WebSocket read error", "error", err)
			}
			break
		}
		var req WSRequest
		if err := json.Unmarshal(message, &req); err != nil {
			if sendErr := client.sendJSON("error", "invalid request"); sendErr != nil {
				slog.Error("Failed to send error response", "error", sendErr)
			}
			continue
		}

		a.handleWSRequest(client, req)
	}

	slog.Info("WebSocket client disconnected")
}

func (a *API) handleWSRequest(client *WSClient, req WSRequest) {
	var resp WSResponse
	resp.Type = req.Type
	slog.Info("Client requested " + req.Type)

	switch req.Type {
	case "get-graph":
		resp.Data = a.GetGraph()
	case "get-armory":
		resp.Data = a.GetArmory("")
	case "get-campaign-state":
		resp.Data = a.GetCampaignState()
	case "get-flow":
		resp.Data = a.GetFlow()
	case "export-attack-flow":
		if flow, err := a.ran.Campaign.GetAuditTrail().ConvertToAttackFlow(); err != nil {
			resp.Error = err.Error()
		} else {
			resp.Data = flow
		}
	case "get-applicable-ttps":
		var params struct {
			TargetID string `json:"targetId"`
		}
		if err := json.Unmarshal(req.Params, &params); err != nil {
			resp.Error = err.Error()
		} else if ttps, err := a.GetApplicableTTPs(params.TargetID); err != nil {
			resp.Error = err.Error()
			resp.Data = []domain.TTP{}
		} else {
			resp.Data = ttps
		}
	case "execute-action":
		var params ExecuteActionCmd
		if err := json.Unmarshal(req.Params, &params); err != nil {
			resp.Error = err.Error()
		} else {
			args := ActionArgs{}
			if params.Args != nil {
				args = *params.Args
			}
			procedureID := ""
			if params.ProcedureId != nil {
				procedureID = *params.ProcedureId
			}
			execSystemId := ""
			if params.ExecSystemId != nil {
				execSystemId = *params.ExecSystemId
			}
			if err := a.ExecuteAction("", params.ActionId, execSystemId, params.TargetId, procedureID, args); err != nil {
				resp.Error = err.Error()
			} else {
				resp.Data = "ok"
			}
		}
	case "reset-campaign":
		if err := a.ResetCampaign(); err != nil {
			resp.Error = err.Error()
		} else {
			resp.Data = "ok"
		}
	case "get-running-pods":
		var params struct {
			Namespace string `json:"namespace"`
		}
		err := json.Unmarshal(req.Params, &params) // ignore error, namespace is optional
		if err != nil {
			slog.Warn("get-running-pods: failed to parse params", "error", err)
		} else {
			if pods, err := a.GetRunningPods(params.Namespace); err != nil {
				resp.Data = err
			} else {
				resp.Data = pods
			}
		}
	default:
		resp.Type = resp.Error
		resp.Error = "unknown request type: " + req.Type
	}

	if err := client.sendJSON(req.Type, resp.Data); err != nil {
		slog.Error("Failed to send response", "error", err)
	}
}
