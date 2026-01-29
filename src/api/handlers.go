package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"time"

	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	"github.com/google/uuid"
)

// HTTPHandler wraps the existing API methods to implement ServerInterface
type HTTPHandler struct {
	api *API
}

// NewHTTPHandler creates a new HTTP handler that implements ServerInterface
func NewHTTPHandler(api *API) *HTTPHandler {
	return &HTTPHandler{api: api}
}

// GetGraph implements ServerInterface
func (h *HTTPHandler) GetGraph(w http.ResponseWriter, r *http.Request) {
	graph := h.api.GetGraph()
	respondJSON(w, http.StatusOK, graph)
}

// GetCampaignState implements ServerInterface
func (h *HTTPHandler) GetCampaignState(w http.ResponseWriter, r *http.Request) {
	state := h.api.GetCampaignState()
	respondJSON(w, http.StatusOK, state)
}

// GetArmory implements ServerInterface
func (h *HTTPHandler) GetArmory(w http.ResponseWriter, r *http.Request, params GetArmoryParams) {
	tactic := ""
	if params.Tactic != nil {
		tactic = *params.Tactic
	}

	armory := h.api.GetArmory(tactic)
	respondJSON(w, http.StatusOK, armory)
}

// GetApplicableTTPs implements ServerInterface
func (h *HTTPHandler) GetApplicableTTPs(w http.ResponseWriter, r *http.Request, params GetApplicableTTPsParams) {
	targetId := ""
	if params.TargetId != nil {
		targetId = *params.TargetId
	}

	ttps, err := h.api.GetApplicableTTPs(targetId)
	if err != nil {
		respondError(w, http.StatusNotFound, err.Error())
		return
	}
	respondJSON(w, http.StatusOK, ttps)
}

// GetFlow implements ServerInterface
func (h *HTTPHandler) GetFlow(w http.ResponseWriter, r *http.Request) {
	flow := h.api.GetFlow()
	respondJSON(w, http.StatusOK, flow)
}

// ExportAttackFlow implements ServerInterface
func (h *HTTPHandler) ExportAttackFlow(w http.ResponseWriter, r *http.Request) {
	flow, err := h.api.ran.Campaign.GetAuditTrail().ConvertToAttackFlow()
	if err != nil {
		respondError(w, http.StatusInternalServerError, err.Error())
		return
	}
	respondJSON(w, http.StatusOK, flow)
}

// SaveFlow implements ServerInterface
func (h *HTTPHandler) SaveFlow(w http.ResponseWriter, r *http.Request) {
	var req SaveFlowJSONRequestBody
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		respondError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	success, err := h.api.SaveFlow(req.Path)
	if err != nil {
		respondError(w, http.StatusBadRequest, err.Error())
		return
	}

	respondJSON(w, http.StatusOK, map[string]bool{"success": success})
}

// ExecuteAction implements ServerInterface
func (h *HTTPHandler) ExecuteAction(w http.ResponseWriter, r *http.Request) {
	var req ExecuteActionJSONRequestBody
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		respondError(w, http.StatusBadRequest, "invalid request body: "+err.Error())
		return
	}

	args := make(map[string]string)
	if req.Args != nil {
		args = *req.Args
	}

	procedureID := ""
	if req.ProcedureId != nil {
		procedureID = *req.ProcedureId
	}

	// Create a channel to signal completion
	done := make(chan domain.Event, 1)

	cmdID := uuid.New().String()
	h.api.ran.Bus.SubscribeUntil(domain.TTPExecuted{},
		func(msg domain.Message) bool {
			// Unsubscribe when we receive the event matching our cmdID
			if event, ok := msg.(domain.TTPExecuted); ok {
				return event.CmdId == cmdID || event.ID == cmdID
			}
			return false
		},
		func(ctx context.Context, msg domain.Message) (domain.Message, error) {
			event, ok := msg.(domain.TTPExecuted)
			if !ok {
				return nil, nil
			}
			if event.CmdId == cmdID || event.ID == cmdID {
				done <- event
			}
			return nil, nil
		},
	)

	// handle error or status depending on execution time
	if err := h.api.ExecuteAction(cmdID, req.ActionId, req.TargetId, procedureID, args); err != nil {
		// Check if it's a NotFoundError and return 404
		var notFoundErr *campaign.NotFoundError
		if errors.As(err, &notFoundErr) {
			respondError(w, http.StatusNotFound, err.Error())
		} else {
			respondError(w, http.StatusBadRequest, err.Error())
		}
		return
	} else {
		ctx, cancel := context.WithTimeout(r.Context(), 15*time.Second)
		defer cancel()

		select {
		case event := <-done:
			// Action completed within 15 seconds
			execEvent, ok := event.(domain.TTPExecuted)
			if !ok {
				respondError(w, http.StatusInternalServerError, "invalid event type received")
			} else if !execEvent.Success {
				failReason := execEvent.FailReason
				if failReason == "" && len(execEvent.Results) > 0 {
					failReason = execEvent.Results[0]
				}
				respondError(w, http.StatusConflict, "action execution failed: "+failReason)
			} else {
				respondJSON(w, http.StatusOK, map[string]string{"status": "ok"})
			}
		case <-ctx.Done():
			// Action taking longer than 15 seconds, return 202 Accepted
			respondJSON(w, http.StatusAccepted, map[string]string{
				"status": "action is still executing",
				"taskId": cmdID,
			})
		}
	}
}

// ResetCampaign implements ServerInterface
func (h *HTTPHandler) ResetCampaign(w http.ResponseWriter, r *http.Request) {
	if err := h.api.ResetCampaign(); err != nil {
		respondError(w, http.StatusInternalServerError, err.Error())
		return
	}

	respondJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

// GetRunningPods implements ServerInterface
func (h *HTTPHandler) GetRunningPods(w http.ResponseWriter, r *http.Request, params GetRunningPodsParams) {
	namespace := ""
	if params.Namespace != nil {
		namespace = *params.Namespace
	}

	pods, err := h.api.GetRunningPods(namespace)
	if err != nil {
		respondError(w, http.StatusInternalServerError, err.Error())
		return
	}

	respondJSON(w, http.StatusOK, pods)
}

// Helper functions

func respondJSON(w http.ResponseWriter, status int, data interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(data)
}

func respondError(w http.ResponseWriter, status int, message string) {
	respondJSON(w, status, Error{Error: message})
}
