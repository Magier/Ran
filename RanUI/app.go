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
		runtime.LogInfo(a.ctx, "get-armory")
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
	runtime.LogInfo(a.ctx, "DOM ready")

	// graph := a.GetGraph()
	// runtime.EventsEmit(a.ctx, "graph", graph)

	a.ran.ReplayEvents()
}

func (a *App) GetGraph() Graph {
	runtime.LogInfo(a.ctx, "GetGraph")

	// frontend uses this information for compound nodes
	parentNodes := make(map[string]string)

	relations := a.ran.Campaign.GetRelations()
	edges := make([]Edge, 0, len(relations))
	for id, relation := range relations {

		name := relation.GetRelationName()
		// convert "hierarchical" relations to parent relationships.
		// || name == "owns"
		if name == "contains" {
			parentNodes[relation.GetTargetId()] = relation.GetSourceId()
		} else {
			edges = append(edges, Edge{
				ID:       id,
				Name:     name,
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
func (a *App) StartEmulation(target string) bool {
	a.ran.SetTarget(target)
	// a.ran.Start(false, "../campaign_2025-03-03T06-31-16.json")
	return true
}

func (a *App) ActionSelected(actionID, targetID, variant string) { //, args map[string]string) {
	runtime.LogInfo(a.ctx, "ActionSelected"+actionID+" target: "+targetID)
	err := a.ran.Bus.Publish(domain.ActionSelected{
		ActionID: actionID,
		TargetID: targetID,
		Variant:  variant,
		// Args:     args,
	})
	if err != nil {
		runtime.LogError(a.ctx, "failed to publish ActionSelected event: "+err.Error())
	}
}
