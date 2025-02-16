package domain

import (
	"context"
	"fmt"
	"net"
	"strings"

	k8s_types "github.com/Magier/Ran/k8sclient/types"
)

type MessageHandler func(ctx context.Context, msg Message) (Message, error)
type EventHandler func(ctx context.Context, event Event) (Message, error)
type CommandHandler func(ctx context.Context, command Command) (Message, error)

type Event interface {
	Message
	IsEvent()
}
type EventImpl struct {
	CmdId string
}

func (e EventImpl) IsEvent() {}
func (e EventImpl) GetCmdID() string {
	return e.CmdId
}

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
	ActionID string
	TargetID string
	Args     map[string]string
}

var _ Event = (*ActionSelected)(nil)

func (e ActionSelected) String() string {
	return fmt.Sprintf("Action '%s' selected", e.ActionID)
}

type FactsChanged struct {
	EventImpl
	NewEntities       []Entity
	NewRelations      []Relation
	NewIdentities     []Identity
	NewAssets         []Asset
	RemovedEntities   []Entity
	RemovedRelations  []Relation
	RemovedIdentities []Identity
	RemovedAssets     []Asset
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
	newFacts := summarizeChanges(e.NewEntities, e.NewRelations, e.NewIdentities, e.NewAssets)
	if newFacts != "" {
		s += newFacts
	}

	removedFacts := summarizeChanges(e.NewEntities, e.NewRelations, e.NewIdentities, e.NewAssets)
	if removedFacts != "" {
		s += removedFacts
	}
	return s
}

type KnowledgeUpdated struct {
	EventImpl
	NumChanges int
}

func (e KnowledgeUpdated) String() string {
	return fmt.Sprintf("%d facts changed in knowledge base", e.NumChanges)
}

type ServiceAccountTokenExtracted struct {
	EventImpl
	SourceSystemId string
	Token          string
}

func (e ServiceAccountTokenExtracted) String() string {
	return "SA Token extracted"
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
	Target     Entity
	ResultType Message
	Results    []any
}

func (ttp TTPExecuted) String() string {
	return fmt.Sprintf("TTP '%s' executed (%s)", ttp.ID, ttp.TTP.Name)
}

type TTPFailed struct {
	EventImpl
	ID     string
	Reason string
	TTP    TTP
}

func (ttp TTPFailed) String() string {
	return fmt.Sprintf("TTP '%s' failed", ttp.ID)
}

type TokenPermissionsRetrieved struct {
	EventImpl
	TokenName        string
	ServiceAccount   ServiceAccount
	Result           string
	ResourceRules    []k8s_types.ResourceRule
	NonResourceRules []k8s_types.NonResourceRule
}

func (e TokenPermissionsRetrieved) String() string {
	return fmt.Sprintf("'%s' Token Permissions Retrieved", e.TokenName)
}

type GraphRendered struct {
	EventImpl
	Path string
}

func (e GraphRendered) String() string {
	return fmt.Sprintf("saved graph %s ", e.Path)
}
