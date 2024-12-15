package domain

import (
	"fmt"
)

type Command interface {
	Message
	IsCommand()
}
type CommandImpl struct{}

func (c CommandImpl) IsCommand() {}

type TTPParams interface{}

type ResultHandler = func(source Entity, args ...any) (Event, error)

type Requirements struct {
	Kind           string
	AccessLevel    AccessLevel
	RbacPermission string
}

type TTP struct {
	ID          string
	Name        string
	Description string
	Tactics     []string
	Technique   []string

	References []string // ms_id::String = ""

	Cmd  string
	Args []string
	Port uint

	Command   Command
	CommandFn func(TTP) Message

	TargetId        string
	Target          string
	TargetNamespace string

	// Requires      map[string]string
	Requires      Requirements
	Effect        func() string
	Effects       []string
	ResultHandler ResultHandler
	Params        TTPParams

	// action::Union{String,Function,Nothing} = nothing
	// cmd_args::Union{String,Nothing} = nothing
}

func (ttp TTP) GetTitle() string {
	return ttp.Name
}
func (ttp TTP) GetDescription() string {
	return ttp.Description
}
func (ttp TTP) GetMessage() Message {
	if ttp.CommandFn != nil {
		return ttp.CommandFn(ttp)
	} else {
		return ttp.Command
	}
}

func (ttp TTP) HandleResult(source Entity, args ...any) (Event, error) {
	if ttp.ResultHandler == nil {
		return nil, nil
	}
	return ttp.ResultHandler(source, args...)
}

type Templater interface {
	GetTemplate() string
	GroundCommand(string) Templater
}

type Targeter interface {
	GetTarget() Target
	SetTarget(Entity) Target
	// InitTarget(Entity)
}
type Target struct {
	Entity Entity
	Id     string
	Name   string
	Ns     string
}

func (t Target) InitTarget(e Entity) Target {
	newTarget := Target{
		Id:     e.GetId(),
		Name:   e.GetName(),
		Entity: e,
	}

	if nsEntity, ok := e.(Namespaced); ok {
		newTarget.Ns = nsEntity.GetNamespace()
	}
	return newTarget
}

func (t Target) GetTarget() Target {
	return t
}

func (t Target) SetTarget(e Entity) Target {
	t.Id = e.GetId()
	t.Name = e.GetName()
	t.Entity = e

	if nsEntity, ok := e.(Namespaced); ok {
		t.Ns = nsEntity.GetNamespace()
	}
	return t
}

type StartC2 struct {
	CommandImpl
	C2Name string
}

func (cmd StartC2) String() string {
	return "start c2 " + cmd.C2Name
}

type StartListener struct {
	CommandImpl
	Port     uint
	Protocol Protocol
	Server   string
}

func (c StartListener) String() string {
	return fmt.Sprintf("Start Listener on port %d", c.Port)
}

type StopListener struct {
	CommandImpl
	Port     uint
	Protocol Protocol
	Server   string
}

func (c StopListener) String() string {
	return fmt.Sprintf("Stop Listener on port %d", c.Port)
}

type StartC2Redirector struct {
	CommandImpl
	DstPort uint
}

func (c StartC2Redirector) String() string {
	return "Start C2 redirector"
}

type ReadEnvVars struct {
	CommandImpl
	Target
}

func (c ReadEnvVars) String() string {
	return "Read environment variables"
}

type ExecTTP struct {
	CommandImpl
	TTP       TTP
	Cmd       string
	Args      []string
	C2Channel C2Channel
	Target
}

func (e ExecTTP) GetTarget() Target {
	return e.Target
}

func (e ExecTTP) String() string {
	var target string
	if e.C2Channel != nil {
		target = e.C2Channel.GetTarget()
	} else {
		target = e.Target.Id
	}

	return fmt.Sprintf("Executed '%s' on %s", e.Cmd, target)
}

func (e ExecTTP) GetTemplate() string {
	return e.Cmd
}

func (e ExecTTP) GroundCommand(value string) Templater {
	e.Cmd = value
	return e
}

type KubectlExec struct {
	CommandImpl
}

type PrintGraph struct {
	CommandImpl
}

func (p PrintGraph) String() string {
	return "printGraph"
}
