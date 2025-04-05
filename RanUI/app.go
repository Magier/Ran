package main

import (
	"context"
	"encoding/json"
	"fmt"

	ran "github.com/Magier/Ran/core"
	domain "github.com/Magier/Ran/domain"
	"github.com/wailsapp/wails/v2/pkg/runtime"
)

// App struct
type App struct {
	ctx context.Context
	ran *ran.Ran
}

type Node struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	Kind        string `json:"kind"`
	ParentID    string `json:"parent"`
	IP          string `json:"ip"`
	Username    string `json:"username"`
	AccessLevel string `json:"accessLevel"`
	OS          string `json:"os"`
	Version     string `json:"version"`
}

type Edge struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	SourceID string `json:"sourceId"`
	TargetID string `json:"targetId"`
}

type Graph struct {
	Nodes      []Node `json:"nodes"`
	Edges      []Edge `json:"edges"`
	RootNodeID string `json:"rootNodeId"`
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
	r := ran.InitRan("", "armory/", "sliver_cfg.json")

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

	return a
}

// startup is called when the app starts. The context is saved
// so we can call the runtime methods
func (a *App) startup(ctx context.Context) {
	a.ctx = ctx

	runtime.EventsOn(a.ctx, "get-armory", func(data ...any) {
		// runtime.EventsEmit(a.ctx, "armory", "armory")
		// runtime.EventsEmit(a.ctx, "armory", a.ran.Armory.GetTTPs())
		runtime.EventsEmit(a.ctx, "armory-loaded", a.ran.Armory.GetTTPs())
	})

	// runtime.EventsOn(a.ctx, "runtime:ready", func(data ...any) {
	// 	runtime.LogInfo(a.ctx, "Runtime ready")
	// })
	runtime.LogInfo(a.ctx, "RanUI starting up")
	a.ran.Start(a.ctx, false, "")
	// a.ran.Start(false, "../campaign_2025-03-03T06-31-16.json")
}

func (a *App) domready(ctx context.Context) {
	// graph := a.GetGraph()
	// runtime.EventsEmit(a.ctx, "graph", graph)

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
		default:
			edges = append(edges, Edge{
				ID:       id,
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
				parent = "ns/" + nsEntity.GetNamespace()
			}
		}

		node := Node{
			ID:       entity.GetId(),
			Name:     entity.GetName(),
			Kind:     entity.GetKind(),
			ParentID: parent,
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

// Greet returns a greeting for the given name
func (a *App) StartEmulation(target string) error {
	err := a.ran.SetTarget(target)
	// a.ran.Start(false, "../campaign_2025-03-03T06-31-16.json")

	if err != nil {
		runtime.EventsEmit(a.ctx, "error", err.Error())
		result, err := runtime.MessageDialog(a.ctx, runtime.MessageDialogOptions{
			Type:    runtime.QuestionDialog,
			Title:   "Question",
			Message: "Target pod not found. Create the it instead?",
			Buttons: []string{"Yes", "No"},
			// DefaultButton: "No",
		})

		runtime.LogInfo(a.ctx, "Dialog result: "+result)

		return err
	}

	return err
}

func (a *App) ActionSelected(actionID, targetID string, args ActionArgs) { //, args map[string]string) {
	runtime.LogInfo(a.ctx, "ActionSelected"+actionID+" target: "+targetID)
	err := a.ran.Bus.Publish(domain.ActionSelected{
		ActionID: actionID,
		TargetID: targetID,
		// Variant:  variant,
		Args: args,
	})
	if err != nil {
		runtime.LogError(a.ctx, "failed to publish ActionSelected event: "+err.Error())
	}
}

func (a *App) IsActionSatisfied(actionId, targetId string) (bool, error) {
	ttp, ok := a.ran.Armory.GetTTP(actionId)
	if !ok {
		return false, fmt.Errorf("TTP '%s' not found", actionId)
	}

	target, _ := a.ran.Campaign.GetEntityById(targetId)
	state := domain.State{}
	accessLevel := domain.UserExec
	isSatisfied := ttp.Requires.Satisfied(target, accessLevel, state)
	return isSatisfied, nil
}

func (a *App) GetTrace() Graph {

	nodes := make([]Node, 0)
	edges := make([]Edge, 0)

	trail := a.ran.Campaign.GetAuditTrail()

	var srcId string

	for _, step := range trail.GetSteps() {
		node := Node{
			ID:   step.ID,
			Name: step.TTP.Name,
		}
		nodes = append(nodes, node)

		if srcId != "" {
			edges = append(edges, Edge{
				ID:       fmt.Sprintf("%s->%s", srcId, node.ID),
				Name:     "",
				SourceID: srcId,
				TargetID: node.ID,
			})
		}
		srcId = node.ID
	}

	graph := Graph{
		Nodes: nodes,
		Edges: edges,
	}
	return graph
}

func (a *App) GetRunningPods() []string {
	var ns string // empty NS = all namespaces
	client, err := k8s.NewK8sClient("")
	if err != nil {
		runtime.LogErrorf(a.ctx, "Could not get running pods: %v", err)
	}
	pods, err := client.GetPods(a.ctx, ns)
	if err != nil {
		runtime.LogErrorf(a.ctx, "Could not get running pods: %v", err)
	}

	podIds := []string{}
	hiddenNamespaces := []string{"kube-system", "local-path-storage"}

	for _, p := range pods {
		if !slices.Contains(hiddenNamespaces, p.GetNamespace()) {
			podIds = append(podIds, p.GetId())
		}
	}
	// TODO: find a good way to sort the Pods
	return podIds
}
