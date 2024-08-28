package domain

import (
	"context"
	"fmt"
)

type EventHandler func(ctx context.Context, event Event) (Message, error)
type CommandHandler func(ctx context.Context, command Command) (Message, error)

type Event interface {
	Message
	String() string
}

type UiEvent interface {
	UiMessage() string
}

type NewEntities struct {
	Pods []Pod
}

func (n NewEntities) MessageName() string {
	return "NewEntities"
}
func (n NewEntities) String() string {
	return fmt.Sprintf("%d new entities", len(n.Pods))
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
