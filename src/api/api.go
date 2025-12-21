package api

import (
	"context"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/Magier/Ran/campaign"
	ran "github.com/Magier/Ran/core"
	"github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
	"github.com/go-chi/chi/v5"
)

type CampaignState struct {
	Entities  map[string]domain.Entity `json:"entities"`
	Relations []domain.Relation        `json:"relations"`
}

type AttackStep = campaign.AttackStep
type AttackFlow struct {
	Steps      []AttackStep `json:"steps"`
	Edges      []Edge       `json:"edges"`
	RootNodeID string       `json:"rootNodeId"`
}

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

type Node struct {
	ID          string        `json:"id"`
	Name        string        `json:"name"`
	Kind        string        `json:"kind"`
	ParentID    string        `json:"parent"`
	AccessLevel string        `json:"accessLevel"`
	Entity      domain.Entity `json:"entity"`
	Compromised bool          `json:"compromised"`
}

type Edge struct {
	ID       string          `json:"id"`
	Name     string          `json:"name"`
	Weight   float32         `json:"weight"`
	Relation domain.Relation `json:"relation"`
	SourceID string          `json:"sourceId"`
	TargetID string          `json:"targetId"`
}
type Graph struct {
	Nodes      []Node `json:"nodes"`
	Edges      []Edge `json:"edges"`
	RootNodeID string `json:"rootNodeId"`
}

type ExecuteActionCmd struct {
	ActionID    string            `json:"actionId"`
	TargetID    string            `json:"targetId"`
	ProcedureID string            `json:"procedureId"`
	Args        map[string]string `json:"args"`
}

type API struct {
	ctx       context.Context
	ran       *ran.Ran
	clients   map[*WSClient]bool
	clientsMu sync.RWMutex
	router    chi.Router
}

func NewAPI(r *ran.Ran, ctx context.Context) *API {
	a := &API{
		ctx:     ctx,
		ran:     r,
		clients: make(map[*WSClient]bool),
	}
	a.router = chi.NewRouter()

	workDir, _ := os.Getwd()
	frontend := http.Dir(filepath.Join(workDir, "..", "frontend", "build"))
	FileServer(a.router, "/", frontend)

	// router.Get("/graph", func(w http.ResponseWriter, req *http.Request) {
	// 	graph := a.GetGraph()
	// 	w.Header().Set("Content-Type", "application/json")
	// 	json.NewEncoder(w).Encode(graph)
	// })

	a.router.Get("/ws", a.handleWebSocket)

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
	for client := range a.clients {
		if err := client.sendJSON(eventName, msg); err != nil {
			slog.Error("WebSocket send error", "error", err)
		}
	}
	return nil, nil
}

func (a *API) StartServer(addr string) error {
	slog.Info("Starting HTTP server", "port", addr)
	// go func() {
	// 	<-ctx.Done()
	// }()
	return http.ListenAndServe(addr, a.router)
}

func (a *API) SetContext(ctx context.Context) {
	a.ctx = ctx
}

func (a *API) BroadcastMessage(message []byte) {
	for client := range a.clients {
		if err := client.sendJSON("broadcast", message); err != nil {
			slog.Error("WebSocket broadcast error", "error", err)
			// client.Close()
			delete(a.clients, client)
		}
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
		case domain.Contains, domain.ManagesNode:
			parentNodes[relation.GetTargetId()] = relation.GetSourceId()
		case domain.Runs:
			// skip this relation for now, as it's the inverse of RunsOn and adds no uX improvements
		case domain.ExposesSecret:
			// skip this relation for now, because secrets are not shown in the graph
		default:
			edges = append(edges, Edge{
				ID:       id,
				Relation: relation,
				Name:     relation.GetRelationName(),
				SourceID: relation.GetSourceId(),
				TargetID: relation.GetTargetId(),
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
			ID:       entity.GetId(),
			Name:     entity.GetName(),
			Kind:     entity.GetKind(),
			ParentID: parent,
			Entity:   entity,
		}

		switch e := entity.(type) {
		case domain.Pod:
			node.Compromised = e.AccessLevel.IsSet()
		case domain.K8sNode:
			node.Compromised = e.AccessLevel.IsSet()
		case domain.ServiceAccount:
			node.Compromised = (e.Token.Raw != "")
		}

		nodes = append(nodes, node)
	}

	graph := Graph{
		RootNodeID: "c2/Ran",
		Nodes:      nodes,
		Edges:      edges,
	}
	return graph
}

func (a *API) GetCampaignState() CampaignState {
	entitiesMap := a.ran.Campaign.GetEntities()
	entities := make(map[string]domain.Entity, len(entitiesMap))
	for _, entity := range entitiesMap {
		entities[entity.GetId()] = entity
	}
	relationsMap := a.ran.Campaign.GetRelations()
	relations := make([]domain.Relation, 0, len(relationsMap))
	for _, relation := range relationsMap {
		relations = append(relations, relation)
	}
	return CampaignState{
		Entities:  entities,
		Relations: relations,
	}
}

func (a *API) ResetCampaign() error {
	err := a.ran.Bus.Publish(domain.ResetCampaign{})
	if err != nil {
		return fmt.Errorf("failed to reset campaign: %v", err)
	}
	return nil
}

func (a *API) ExecuteAction(actionID, targetID, procedureID string, args ActionArgs) error { //, args map[string]string) {
	err := a.ran.Bus.Publish(domain.ActionSelected{
		ActionID:    actionID,
		TargetID:    targetID,
		ProcedureID: procedureID,
		Args:        args,
	})
	if err != nil {
		return fmt.Errorf("failed to publish ActionSelected event: %s", err.Error())
	}
	return nil
}

func (a *API) GetArmory() []domain.TTP {
	return a.ran.Armory.GetTTPs()
}

func (a *API) GetApplicableTTPs(targetId string) ([]domain.TTP, error) {
	ttps := make([]domain.TTP, 0)
	target, ok := a.ran.Campaign.GetEntityById(targetId)
	if !ok {
		return ttps, fmt.Errorf("failed to get target entity: %s", targetId)
	}
	state := domain.State{}
	accessLevel := domain.UserExec

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
			ttps = append(ttps, ttp)
		}
	}
	return ttps, nil
}

func (a *API) GetFlow() AttackFlow {
	steps := make([]AttackStep, 0)
	edges := make([]Edge, 0)

	trail := a.ran.Campaign.GetAuditTrail()

	var srcId string
	for _, step := range trail.GetSteps() {
		steps = append(steps, step)

		if srcId != "" {
			edges = append(edges, Edge{
				ID:       fmt.Sprintf("%s->%s", srcId, step.ID),
				Name:     "",
				SourceID: srcId,
				TargetID: step.ID,
			})
		}

		// update the srcId for the next edge, if it was a success
		if step.Success {
			srcId = step.ID
		}
	}

	return AttackFlow{
		Steps: steps,
		Edges: edges,
	}
}

func (a *API) GetFacts() domain.FactsChanged { return domain.FactsChanged{} }

type K8sResource struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Namespace string `json:"namespace"`
	Kind      string `json:"kind"`
}

func (a *API) GetRunningPods(ns string) ([]K8sResource, error) {
	ids, err := k8s.GetIDsOfRunningPod(a.ctx, ns)
	if err != nil {
		return nil, err
	}
	resources := make([]K8sResource, 0, len(ids))
	for _, id := range ids {
		ns, kind, name, err := campaign.UnpackResourceID(id)
		if err != nil {
			return nil, fmt.Errorf("Could not unpack resource ID: %v", err)
		} else {
			resources = append(resources, K8sResource{
				ID:        id,
				Name:      name,
				Namespace: ns,
				Kind:      kind,
			})
		}
	}
	return resources, nil
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
