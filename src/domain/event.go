package domain

import (
	"context"
	"fmt"
	"strings"
)

type MessageHandler func(ctx context.Context, msg Message) (Message, error)
type EventHandler func(ctx context.Context, event Event) (Message, error)
type CommandHandler func(ctx context.Context, command Command) (Message, error)

type Event interface {
	Message
	IsEvent()
}
type EventImpl struct{}

func (e EventImpl) IsEvent() {}

type UiEvent interface {
	UiMessage() string
}

type ErrorLevel string

var (
	LevelDebug ErrorLevel = "DEBUG"
	LevelInfo  ErrorLevel = "INFO"
	LevelWarn  ErrorLevel = "WARN"
	LevelError ErrorLevel = "ERROR"
)

type ErrorMsg struct {
	EventImpl
	Level ErrorLevel
	Msg   string
}

func (e ErrorMsg) String() string {
	return e.Msg
}
func (e ErrorMsg) UiMessage() string {
	return e.Msg
}

type NewFacts struct {
	EventImpl
	Entities   []Entity
	Relations  []Relation
	Identities []Identity
	Assets     []Asset
}

func (e NewFacts) String() string {
	resources := map[string]int{
		"entities":   len(e.Entities),
		"relations":  len(e.Relations),
		"identities": len(e.Identities),
		"assets":     len(e.Assets),
	}

	infos := []string{}
	for label, count := range resources {
		if count > 0 {
			infos = append(infos, fmt.Sprintf("%d %s", count, label))
		}
	}
	return "Received new facts: " + strings.Join(infos, ", ")
}

type KnowledgeUpdated struct {
	EventImpl
	NumChanges int
}

func (e KnowledgeUpdated) String() string {
	return fmt.Sprintf("%d facts changed in knowledge base", e.NumChanges)
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
