package api

import (
	"context"
	"embed"
	"fmt"
	"io/fs"
	"log/slog"
	"net/http"
	"strings"
	"sync"
	"time"

	ran "github.com/Magier/Ran/core"
	"github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"
)

// Type aliases for backward compatibility
type ActionArgs map[string]string

type AccessLevel string

const (
	NoAccess AccessLevel = "NoAccess"
	UserRead AccessLevel = "UserRead"
	UserExec AccessLevel = "UserExec"
	RootRead AccessLevel = "RootRead"
	RootExec AccessLevel = "RootExec"
)

var AllAccessLevels = []struct {
	Value  AccessLevel
	TSName string
}{
	{NoAccess, "NoAccess"},
	{UserRead, "UserRead"},
	{UserExec, "UserExec"},
	{RootRead, "RootRead"},
	{RootExec, "RootExec"},
}

type Client interface {
	sendJSON(name string, v interface{}) error
	Close() error
}

type API struct {
	ctx            context.Context
	ran            *ran.Ran
	clients        map[Client]bool
	clientsMu      sync.RWMutex
	router         chi.Router
	podWatchCancel context.CancelFunc
	podWatchMu     sync.Mutex
}

//go:embed all:static
var staticFS embed.FS

func NewAPI(r *ran.Ran, ctx context.Context) *API {
	a := &API{
		ctx:     ctx,
		ran:     r,
		clients: make(map[Client]bool),
	}
	a.router = chi.NewRouter()

	// Add logging middleware to debug requests/responses
	// a.router.Use(middleware.Logger)
	a.router.Use(middleware.Recoverer)
	a.router.Use(middleware.RequestID)

	// Register OpenAPI REST endpoints
	httpHandler := NewHTTPHandler(a)
	HandlerFromMux(httpHandler, a.router)

	// WebSocket endpoint for real-time updates
	a.router.Get("/ws", a.handleWebSocket)

	// SSE endpoint for real-time updates
	a.router.Get("/events", a.handleSSE)

	// Serve OpenAPI spec
	a.router.Get("/api/openapi.yaml", serveOpenAPISpec)

	// Serve Swagger UI
	a.router.Get("/api/docs", serveSwaggerUI)

	// if Vite dev server is running, use it to serve frontend assets, otherwise serve compiled assets
	if IsViteServerRunning() {
		slog.Info("Vite dev server detected, using it to serve frontend assets")
		// Proxy to Vite dev server for frontend assets and routes in non-production
		a.router.Get("/", func(w http.ResponseWriter, r *http.Request) {
			viteProxy := GetViteProxy()
			// Proxy to Vite for frontend assets and routes
			viteProxy.ServeHTTP(w, r)
		})
		a.router.Handle("/*", GetViteProxy())
	} else {
		slog.Debug("Serving static assets from embedded filesystem")
		static, _ := fs.Sub(staticFS, "static")
		FileServer(a.router, "/", http.FS(static))
	}

	// forward all events directly to the frontend
	r.Bus.SubscribeToName(domain.ALL_EVENTS, a.handleEvent)
	return a
}

// FileServer conveniently sets up a http.FileServer handler to serve
// static files from a http.FileSystem.
func FileServer(r chi.Router, path string, root http.FileSystem) {
	if strings.ContainsAny(path, "{}*") {
		panic("FileServer does not permit any URL parameters.")
	}

	if path != "/" && path[len(path)-1] != '/' {
		r.Get(path, http.RedirectHandler(path+"/", 301).ServeHTTP)
		path += "/"
	}
	path += "*"

	r.Get(path, func(w http.ResponseWriter, r *http.Request) {
		rctx := chi.RouteContext(r.Context())
		pathPrefix := strings.TrimSuffix(rctx.RoutePattern(), "/*")
		fs := http.StripPrefix(pathPrefix, http.FileServer(root))
		fs.ServeHTTP(w, r)
	})
}

func (a *API) handleEvent(ctx context.Context, msg domain.Message) (domain.Message, error) {
	eventName := domain.CleanEventName(fmt.Sprintf("%T", msg))
	slog.Info(">> 🖥️: " + eventName)

	a.clientsMu.RLock()
	clients := make([]Client, 0, len(a.clients))
	for client := range a.clients {
		clients = append(clients, client)
	}
	a.clientsMu.RUnlock()

	for _, client := range clients {
		if err := client.sendJSON(eventName, msg); err != nil {
			slog.Error("WebSocket send error", "error", err)
		}
	}
	return nil, nil
}

func (a *API) StartServer(ctx context.Context, addr string) error {
	slog.Info("Starting HTTP server", "port", addr)
	server := &http.Server{
		Addr:    addr,
		Handler: a.router,
	}

	// Start server in a goroutine
	go func() {
		if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			slog.Error("HTTP server error", "error", err)
		}
	}()

	// Wait for context cancellation
	<-ctx.Done()
	slog.Info("Shutting down HTTP server...")

	// Create a deadline for shutdown
	shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	// Gracefully shutdown the server
	if err := server.Shutdown(shutdownCtx); err != nil {
		slog.Error("Server forced to shutdown", "error", err)
		return err
	}

	slog.Info("Server stopped gracefully")
	return nil
}

func (a *API) SetContext(ctx context.Context) {
	a.ctx = ctx
}

func (a *API) BroadcastMessage(message []byte) {
	a.clientsMu.RLock()
	clients := make([]Client, 0, len(a.clients))
	for client := range a.clients {
		clients = append(clients, client)
	}
	a.clientsMu.RUnlock()

	var failedClients []Client
	for _, client := range clients {
		if err := client.sendJSON("broadcast", message); err != nil {
			slog.Error("WebSocket broadcast error", "error", err)
			failedClients = append(failedClients, client)
		}
	}

	if len(failedClients) > 0 {
		a.clientsMu.Lock()
		for _, client := range failedClients {
			delete(a.clients, client)
		}
		a.clientsMu.Unlock()
	}
}

func (a *API) ClientReady(ctx context.Context) {
	a.ran.ReplayEvents()
}

func (a *API) GetGraph() Graph {
	// frontend uses this information for compound nodes
	parentNodes := make(map[string]string)

	relations := a.ran.Campaign.GetRelations()
	edges := make([]Edge, 0, len(relations))
	for id, relation := range relations {
		switch relation.(type) {
		// convert specific "hierarchical" relations to parent relationships.
		case domain.ManagesNode, domain.Owns:
			parentNodes[relation.GetTargetId()] = relation.GetSourceId()
		case domain.Contains:
			// Contains relation has the lowest priority, any other more explicit relation wins
			if parentNodes[relation.GetTargetId()] == "" {
				parentNodes[relation.GetTargetId()] = relation.GetSourceId()
			}
		case domain.Runs:
			// skip this relation for now, as it's the inverse of RunsOn and adds no uX improvements
		case domain.ExposesSecret:
			// skip this relation for now, because secrets are not shown in the graph
		default:
			edges = append(edges, Edge{
				Id:       id,
				Name:     relation.GetRelationName(),
				SourceId: relation.GetSourceId(),
				TargetId: relation.GetTargetId(),
			})
		}
	}

	entities := a.ran.Campaign.GetEntities()
	nodes := make([]Node, 0, len(entities))
	for _, entity := range entities {
		// Use the entity's ID as the Node ID.
		// Here, we're using the entity's type (via %T) as its Name.
		// Adjust this logic if your entity has a dedicated name field.
		parent := parentNodes[entity.GetId()]

		if parent == "" {
			if nsEntity, ok := entity.(domain.Namespaced); ok {
				ns := nsEntity.GetNamespace()
				// only assign parent if the namespace is known
				if ns != "" {
					parent = "ns/" + ns
				} else {
					parent = "cluster" // the resource must be part of the cluster
				}
			}
		}

		node := Node{
			Id:       entity.GetId(),
			Name:     entity.GetName(),
			Kind:     entity.GetKind(),
			EntityId: entity.GetId(),
		}
		if parent != "" {
			node.Parent = &parent
		}

		switch e := entity.(type) {
		case domain.Pod:
			if e.AccessLevel.IsSet() {
				comp := true
				node.Compromised = &comp
			}
		case domain.K8sNode:
			if e.AccessLevel.IsSet() {
				comp := true
				node.Compromised = &comp
			}
		case domain.ServiceAccount:
			if e.Token.Raw != "" {
				comp := true
				node.Compromised = &comp
			}
		}

		nodes = append(nodes, node)
	}

	graph := Graph{
		RootNodeId: "c2/Ran",
		Nodes:      nodes,
		Edges:      edges,
	}
	return graph
}

func (a *API) ResetCampaign() error {
	err := a.ran.Bus.Publish(domain.ResetCampaign{})
	if err != nil {
		return fmt.Errorf("failed to reset campaign: %v", err)
	}
	return nil
}

func (a *API) ExecuteAction(cmdID, actionID, execSystemId, targetID, procedureID string, args ActionArgs) error { //, args map[string]string) {
	err := a.ran.Campaign.ExecuteAction(a.ctx, domain.ActionSelected{
		EventImpl:    domain.EventImpl{CmdId: cmdID},
		ActionID:     actionID,
		ExecSystemID: execSystemId,
		TargetID:     targetID,
		ProcedureID:  procedureID,
		Args:         args,
	})
	return err
}

func (a *API) GetArmory(tactic string) []TTP {
	ttps := make([]TTP, 0)
	for _, ttp := range a.ran.Armory.GetTTPs() {
		if tactic == "" || string(ttp.Tactic) == tactic {
			ttps = append(ttps, ConvertTTP(ttp))
		}
	}
	return ttps
}

func (a *API) GetApplicableTTPs(targetId string) ([]TTP, error) {
	ttps := make([]TTP, 0)
	target, ok := a.ran.Campaign.GetEntityById(targetId)
	if !ok {
		return ttps, fmt.Errorf("failed to get target entity: %s", targetId)
	}
	state := domain.State{}
	accessLevel := domain.NoAccess

	if sys, ok := target.(domain.System); ok {
		accessLevel = sys.GetAccessLevel()
	}

	// TODO: this uses RBACPerm.String() to check for equality, however, it does not consider the scope of the permission
	// Implement a more robust way to check "satisfaction" of permissions, that supports scope and wildcards
	identities := a.ran.Campaign.GetIdentities()
	entitlements := make(map[string][]string)
	for _, identity := range identities {
		for _, e := range identity.GetEntitlements() {
			ids := []string{}
			if existingIds, ok := entitlements[e.String()]; ok {
				ids = existingIds
			}
			// TODO: some system:authorized entitlements are double per identity
			ids = append(ids, identity.GetId())
			entitlements[e.String()] = ids
		}
	}
	state.Entitlements = entitlements

	for _, ttp := range a.ran.Armory.GetTTPs() {
		isSatisfied := ttp.Requires.Satisfied(target, accessLevel, state)
		if isSatisfied && ttp.Status != "disabled" {
			ttps = append(ttps, ConvertTTP(ttp))
		}
	}
	return ttps, nil
}

func (a *API) GetFlow() AttackFlow {
	steps := make([]AttackStep, 0)
	edges := make([]Edge, 0)

	trail := a.ran.Campaign.GetAuditTrail()

	var srcId string
	for _, step := range trail.GetSteps(true) {
		s := ConvertAttackStep(step)
		steps = append(steps, s)

		if srcId != "" {
			edges = append(edges, Edge{
				Id:       fmt.Sprintf("%s->%s", srcId, step.ID),
				Name:     "",
				SourceId: srcId,
				TargetId: step.ID,
			})
		}

		// update the srcId for the next edge, if it was a success
		if step.Status == domain.StepStatusSuccess {
			srcId = step.ID
		}
	}

	return AttackFlow{
		Steps: steps,
		Edges: edges,
	}
}

func (a *API) GetFacts() domain.FactsChanged { return domain.FactsChanged{} }

func (a *API) GetRunningPods(ns string) ([]K8sResource, error) {
	statuses, err := k8s.GetPodStatuses(a.ctx, ns)
	if err != nil {
		return nil, err
	}
	resources := make([]K8sResource, 0, len(statuses))
	for _, s := range statuses {
		ns := s.Namespace
		stateReason := s.StateReason
		resources = append(resources, K8sResource{
			Id:          s.Id,
			Name:        s.Name,
			Namespace:   &ns,
			Kind:        "pod",
			Phase:       &s.Phase,
			Ready:       &s.Ready,
			StateReason: &stateReason,
		})
	}
	return resources, nil
}

func (a *API) StartPodWatch(namespace string) error {
	a.podWatchMu.Lock()
	defer a.podWatchMu.Unlock()

	// Stop any existing watch first
	if a.podWatchCancel != nil {
		a.podWatchCancel()
		a.podWatchCancel = nil
	}

	watchCtx, cancel := context.WithCancel(a.ctx)
	a.podWatchCancel = cancel

	go func() {
		defer func() {
			a.podWatchMu.Lock()
			a.podWatchCancel = nil
			a.podWatchMu.Unlock()
		}()

		err := k8s.WatchPods(watchCtx, namespace, func(pods []domain.PodStatus) {
			a.ran.Bus.Publish(domain.PodsChanged{Pods: pods})
		})
		if err != nil && err != context.Canceled {
			slog.Error("Pod watch ended with error", "error", err)
		}
	}()

	slog.Info("Started pod watch", "namespace", namespace)
	return nil
}

func (a *API) StopPodWatch() {
	a.podWatchMu.Lock()
	defer a.podWatchMu.Unlock()

	if a.podWatchCancel != nil {
		a.podWatchCancel()
		a.podWatchCancel = nil
		slog.Info("Stopped pod watch")
	}
}

func (a *API) SaveFlow(path string) (bool, error) {
	now := time.Now().Format("2006-01-02T15-04-05")
	defaultFileName := fmt.Sprintf("campaign_%s.json", now)
	var _ = defaultFileName

	if path != "" {
		err := a.ran.Bus.Publish(domain.SaveAttackFlow{Path: path})

		if err != nil {
			return false, fmt.Errorf("Failed to save campaign flow: %v", err)
		} else {
			slog.Info("Campaign flow saved successfully", "path", path)
			return true, nil
		}
	} else {
		return false, fmt.Errorf("no file path provided")
	}
	// TODO: send toast to UI if it was successful or not
}

func serveSwaggerUI(w http.ResponseWriter, r *http.Request) {
	html := `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Ran API Documentation</title>
  <link rel="stylesheet" type="text/css" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css" />
  <style>
    html { box-sizing: border-box; overflow: -moz-scrollbars-vertical; overflow-y: scroll; }
    *, *:before, *:after { box-sizing: inherit; }
    body { margin:0; padding:0; }
  </style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-standalone-preset.js"></script>
  <script>
    window.onload = function() {
      window.ui = SwaggerUIBundle({
        url: "/api/openapi.yaml",
        dom_id: '#swagger-ui',
        deepLinking: true,
        presets: [
          SwaggerUIBundle.presets.apis,
          SwaggerUIStandalonePreset
        ],
        plugins: [
          SwaggerUIBundle.plugins.DownloadUrl
        ],
        layout: "StandaloneLayout"
      });
    };
  </script>
</body>
</html>`
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.WriteHeader(http.StatusOK)
	if _, err := w.Write([]byte(html)); err != nil {
		slog.Error("Failed to write Swagger UI HTML response", "error", err)
	}
}

func serveOpenAPISpec(w http.ResponseWriter, r *http.Request) {
	spec, err := GetSwagger()
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	data, err := spec.MarshalJSON()
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	if _, err := w.Write(data); err != nil {
		slog.Error("Failed to write OpenAPI spec response", "error", err)
	}
}
