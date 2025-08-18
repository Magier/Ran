package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"os"
	"time"

	campaign "github.com/Magier/Ran/campaign"
	ran "github.com/Magier/Ran/core"
	domain "github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
	"github.com/wailsapp/wails/v2/pkg/runtime"
)

// App struct
type App struct {
	ctx context.Context
	ran *ran.Ran
}

// type Entitlement struct {
// 	Verbs         []string `json:"verbs"`
// 	ResourceTypes []string `json:"resourceTypes"`
// 	ResourceNames []string `json:"resourceNames"`
// 	Namespace     string   `json:"namespace"`
// }

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

type CampaignState struct {
	Entities  []domain.Entity   `json:"entities"`
	Relations []domain.Relation `json:"relations"`
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

// type TTP struct {
// 	ID          string `json:"id"`
// 	Name        string `json:"name"`
// 	Description string `json:"description"`
// 	Techniques   []string `json:"techniques"`
// 	Tactic      string `json:"tactic"`
// 	Variants     []string `json:"variant"`
// 	Parameters  string `json:"params"`
// }

// NewApp creates a new App application struct
func NewApp() *App {
	r := ran.InitRan("", "armory/")
	a := &App{ran: &r}

	// forward all events directly to the frontend
	r.Bus.SubscribeToName(domain.ALL_EVENTS, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		eventName := domain.CleanEventName(fmt.Sprintf("%T", msg))
		runtime.LogInfo(a.ctx, ">> 🖥️: "+eventName)

		jsonBytes, err := json.Marshal(msg)
		if err != nil {
			runtime.LogError(a.ctx, "failed to marshal event: "+err.Error())
			return nil, err
		}
		runtime.EventsEmit(a.ctx, eventName, string(jsonBytes))
		return nil, nil
	})

	// Initialize structured logging with slog.
	// Make sure to add "os" and "log/slog" to your import list.
	h := slog.NewJSONHandler(os.Stdout, &slog.HandlerOptions{
		Level: slog.LevelInfo,
	})
	slog.SetDefault(slog.New(h))

	return a
}

// func ignoreNestedAttributes(groups []string, a slog.Attr) slog.Attr {
// 	if a.Key == slog.TimeKey ||
// 		a.Key == slog.LevelKey ||
// 		a.Key == slog.MessageKey {
// 		return slog.Attr{}
// 	}
// 	return a
// }

// func NewLogHandler() slog.Handler {
// 	lvl := new(slog.LevelVar)
// 	lvl.Set(slog.LevelInfo)
// 	opts := &slog.HandlerOptions{
// 		AddSource:   false,
// 		ReplaceAttr: ignoreNestedAttributes,
// 		Level:       lvl,
// 	}
// 	b := &bytes.Buffer{}
// 	return &LogHandler{
// 		h:       slog.NewTextHandler(b, opts),
// 		b:       b,
// 		m:       &sync.Mutex{},
// 		program: p,
// 	}
// }

// startup is called when the app starts. The context is saved
// so we can call the runtime methods
func (a *App) startup(ctx context.Context) {
	a.ctx = ctx

	runtime.EventsOn(a.ctx, "get-armory", func(data ...any) {
		runtime.EventsEmit(a.ctx, "armory-loaded", a.ran.Armory.GetTTPs())
	})

	runtime.LogInfo(a.ctx, "RanUI starting up")
	a.ran.Start(a.ctx, false, "")
	// a.ran.Start(false, "../campaign_2025-03-03T06-31-16.json")
}

func (a *App) domready(ctx context.Context) {
	a.ran.ReplayEvents()
}

func (a *App) GetGraph() Graph {
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

func (a *App) GetCampaignState() CampaignState {
	entitiesMap := a.ran.Campaign.GetEntities()
	entities := make([]domain.Entity, 0, len(entitiesMap))
	for _, entity := range entitiesMap {
		entities = append(entities, entity)
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

func (a *App) ResetCampaign() {
	err := a.ran.Bus.Publish(domain.ResetCampaign{})
	if err != nil {
		runtime.LogErrorf(a.ctx, "Failed to reset campaign: %v", err)
		return
	}
}

func (a *App) ExecuteAction(actionID, targetID, procedureID string, args ActionArgs) { //, args map[string]string) {
	runtime.LogInfo(a.ctx, "ActionSelected"+actionID+" target: "+targetID)
	err := a.ran.Bus.Publish(domain.ActionSelected{
		ActionID:    actionID,
		TargetID:    targetID,
		ProcedureID: procedureID,
		Args:        args,
	})
	if err != nil {
		runtime.LogError(a.ctx, "failed to publish ActionSelected event: "+err.Error())
	}
}

func (a *App) GetApplicableTTPs(targetId string) []domain.TTP {
	ttps := make([]domain.TTP, 0)
	target, _ := a.ran.Campaign.GetEntityById(targetId)
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
	return ttps
}

func (a *App) GetFlow() AttackFlow {
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

func (a *App) GetFacts() domain.FactsChanged { return domain.FactsChanged{} }

type K8sResource struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Namespace string `json:"namespace"`
	Kind      string `json:"kind"`
}

func (a *App) GetRunningPods(ns string) []K8sResource {
	ids, err := k8s.GetIDsOfRunningPod(a.ctx, ns)
	if err != nil {
		runtime.LogErrorf(a.ctx, "Could not get running pods: %v", err)
	}
	resources := make([]K8sResource, 0, len(ids))
	for _, id := range ids {
		ns, kind, name, err := campaign.UnpackResourceID(id)
		if err != nil {
			runtime.LogErrorf(a.ctx, "Could not unpack resource ID: %v", err)
		} else {
			resources = append(resources, K8sResource{
				ID:        id,
				Name:      name,
				Namespace: ns,
				Kind:      kind,
			})
		}
	}
	return resources
}

func (a *App) SaveFlow() bool {
	now := time.Now().Format("2006-01-02T15-04-05")
	defaultFileName := fmt.Sprintf("campaign_%s.json", now)
	selection, err := runtime.SaveFileDialog(a.ctx, runtime.SaveDialogOptions{
		Title:                "Save Attack Flow",
		DefaultFilename:      defaultFileName,
		CanCreateDirectories: true,
		Filters: []runtime.FileFilter{
			{
				DisplayName: "Attack Flow (*.json)",
				Pattern:     "*.json",
			},
		},
	})
	if err != nil {
		runtime.LogErrorf(a.ctx, "Failed to open file dialog: %v", err)
	} else {
		if selection != "" {
			err := a.ran.Bus.Publish(domain.SaveAttackFlow{Path: selection})

			if err != nil {
				runtime.LogErrorf(a.ctx, "Failed to save campaign flow: %v", err)
			} else {
				runtime.LogInfof(a.ctx, "Campaign flow saved successfully to %s", selection)
				return true
			}
		} else {
			runtime.LogInfof(a.ctx, "No file selected")
		}
		// TODO: send toast to UI if it was successful or not
	}

	return false
}
