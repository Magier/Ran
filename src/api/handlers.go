package api

import (
	"encoding/json"
	"net/http"
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
func (h *HTTPHandler) GetArmory(w http.ResponseWriter, r *http.Request) {
	armory := h.api.GetArmory()
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
		respondError(w, http.StatusBadRequest, "invalid request body")
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

	if err := h.api.ExecuteAction(req.ActionId, req.TargetId, procedureID, args); err != nil {
		respondError(w, http.StatusBadRequest, err.Error())
		return
	}

	respondJSON(w, http.StatusOK, map[string]string{"status": "ok"})
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
