package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"os"
	"slices"
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
	ID          string `json:"id"`
	Name        string `json:"name"`
	Kind        string `json:"kind"`
	ParentID    string `json:"parent"`
	AccessLevel string `json:"accessLevel"`
	// Entitlements []Entitlement `json:"entitlements"`
	Entity      domain.Entity `json:"entity"`
	Compromised bool          `json:"compromised"`
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
		case domain.Runs:
			// skip this relation for now, as it's the inverse of RunsOn and adds no uX improvements
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
