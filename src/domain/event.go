package domain

import (
	"context"
	"fmt"
	"net"
	"runtime"
	"strings"
)

type MessageHandler func(ctx context.Context, msg Message) (Message, error)
type EventHandler func(ctx context.Context, event Event) (Message, error)
type CommandHandler func(ctx context.Context, command Command) (Message, error)

const ALL_EVENTS = "*"

type Event interface {
	Message
	IsEvent()
}
type EventImpl struct {
	CmdId string
}

func (e EventImpl) IsEvent() {}
func (e EventImpl) GetID() string {
	if e.CmdId == "" {
		pc, _, _, ok := runtime.Caller(1)
		if ok {
			fn := runtime.FuncForPC(pc)
			if fn != nil {
				parts := strings.Split(fn.Name(), ".")
				if len(parts) >= 2 {
					return parts[len(parts)-2]
				}
			}
		}
		return "unknown"
	}

	return e.CmdId
}

type RanReady struct {
	EventImpl
}

func (e RanReady) String() string {
	return "Ran Ready"
}

type CampaignReset struct {
	EventImpl
}

func (r CampaignReset) String() string {
	return "Campaign Reset"
}

var _ Event = (*CampaignReset)(nil)

type UiEvent interface {
	UiMessage() string
}

type MsgLevel string

var (
	LevelDebug MsgLevel = "DEBUG"
	LevelInfo  MsgLevel = "INFO"
	LevelWarn  MsgLevel = "WARN"
	LevelError MsgLevel = "ERROR"
	LevelFatal MsgLevel = "FATAL"
)

type ErrorMsg struct {
	EventImpl
	Level MsgLevel
	Msg   string
}

func (e ErrorMsg) String() string {
	return e.Msg
}
func (e ErrorMsg) UiMessage() string {
	return e.Msg
}

type ActionSelected struct {
	EventImpl
	ActionID    string
	TargetID    string
	ProcedureID string
	Variant     string
	Args        map[string]string
}

var _ Event = (*ActionSelected)(nil)

func (e ActionSelected) String() string {
	return fmt.Sprintf("Action '%s' selected", e.ActionID)
}

type Facts struct {
	Entities   []Entity
	Relations  []Relation
	Identities []Identity
	Assets     []Asset
}

func (f *Facts) Update(new Facts) {
	f.Entities = append(f.Entities, new.Entities...)
	f.Assets = append(f.Assets, new.Assets...)
	f.Relations = append(f.Relations, new.Relations...)
	f.Identities = append(f.Identities, new.Identities...)
}

type FactsChanged struct {
	EventImpl
	New     Facts
	Removed Facts
}

func (e FactsChanged) String() string {
	summarizeChanges := func(entities []Entity, rels []Relation, ids []Identity, assets []Asset) string {
		resources := map[string]int{
			"entities":   len(entities),
			"relations":  len(rels),
			"identities": len(ids),
			"assets":     len(assets),
		}

		infos := []string{}
		for label, count := range resources {
			if count > 0 {
				infos = append(infos, fmt.Sprintf("%d %s", count, label))
			}
		}
		return strings.Join(infos, ", ")
	}

	s := "KB Update: "
	newFacts := summarizeChanges(e.New.Entities, e.New.Relations, e.New.Identities, e.New.Assets)
	if newFacts != "" {
		s += newFacts
	}

	removedFacts := summarizeChanges(e.Removed.Entities, e.Removed.Relations, e.Removed.Identities, e.Removed.Assets)
	if removedFacts != "" {
		s += removedFacts
	}
	return s
}

type ServiceAccountTokenExtracted struct {
	EventImpl
	SourceSystemId string
	Token          string
}

func (e ServiceAccountTokenExtracted) String() string {
	return "SA Token extracted"
}

type NewPodDeployed struct {
	EventImpl
	Pod       Entity
	Namespace Namespace
}

func (e NewPodDeployed) String() string {
	return "New container deployed"
}

type EnvVarsExtracted struct {
	EventImpl
	Source Entity
	Vars   map[string]string
}

func (e EnvVarsExtracted) String() string {
	return fmt.Sprintf("Extracted environment variables from %s", e.Source.GetName())
}

type C2Connected struct {
	EventImpl
	Name string
	IP   net.IP
	Kind string
}

func (e C2Connected) String() string {
	return fmt.Sprintf("Connected to '%s' C2 server on %s", e.Name, e.IP)
}

type TTPExecuted struct {
	EventImpl
	ID         string
	TTP        TTP
	Args       map[string]string
	Procedure  Procedure
	Success    bool
	Target     Entity
	ResultType Message
	Results    []string
	ExecutedOn System
	WasCleanup bool
}

func (ttp TTPExecuted) String() string {
	return fmt.Sprintf("TTP '%s' executed (%s)", ttp.ID, ttp.TTP.Name)
}

// type TokenPermissionsRetrieved struct {
// 	EventImpl
// 	TokenName        string
// 	ServiceAccount   ServiceAccount
// 	Result           string
// 	ResourceRules    []k8s_types.ResourceRule
// 	NonResourceRules []k8s_types.NonResourceRule
// }

// func (e TokenPermissionsRetrieved) String() string {
// 	return fmt.Sprintf("'%s' Token Permissions Retrieved", e.TokenName)
// }

type GraphRendered struct {
	EventImpl
	Path string
}

func (e GraphRendered) String() string {
	return fmt.Sprintf("saved graph %s ", e.Path)
}

type AttackFlowSaved struct {
	EventImpl
	Path string
}

func (e AttackFlowSaved) String() string {
	return fmt.Sprintf("saved attack flow to %s ", e.Path)
}

type PodDeployed struct {
	EventImpl
	PodCfg    PodConfig
	Namespace string
}

type NewRoleBindingCreated struct {
	EventImpl
	Binding        Entity
	Role           string
	ServiceAccount string
	Namespace      Namespace
}

func (e NewRoleBindingCreated) String() string {
	return "New role-binding created"
}
