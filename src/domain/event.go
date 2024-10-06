package domain

import (
	"context"
	"fmt"
)

type EventHandler func(ctx context.Context, event Event) (Message, error)
type CommandHandler func(ctx context.Context, command Command) (Message, error)

type Event interface {
	Message
}

type UiEvent interface {
	UiMessage() string
}

type NewEntities struct {
	Entities   []Entity
	Identities []Identity
}

func (n NewEntities) MessageName() string {
	return "NewEntities"
}
func (n NewEntities) String() string {
	return fmt.Sprintf("%d new entities", len(n.Entities))
}

type ErrorLevel string

var (
	LevelInfo  ErrorLevel = "info"
	LevelWarn  ErrorLevel = "warn"
	LevelError ErrorLevel = "error"
)

type ErrorMsg struct {
	Level ErrorLevel
	Msg   string
}

func (e ErrorMsg) MessageName() string {
	return "ErrorMsg"
}
func (e ErrorMsg) String() string {
	return e.Msg
}
func (e ErrorMsg) UiMessage() string {
	return e.Msg
}

type NewFacts struct {
	Entities  []Entity
	Relations []Relation
	Assets    []Asset
}

func (e NewFacts) MessageName() string {
	return "NewFacts"
}
func (e NewFacts) String() string {
	return fmt.Sprintf("Received new facts: %d entities, %d relatiosn, %d assets", len(e.Entities), len(e.Relations), len(e.Assets))
}

type EnvVarsExtracted struct {
	Source Entity
	Vars   map[string]string
}

func (e EnvVarsExtracted) MessageName() string {
	return "EnvVarsExtracted"
}

func (e EnvVarsExtracted) String() string {
	return fmt.Sprintf("Extracted environment variables from %s", e.Source.GetName())
}
