package domain

import (
	"fmt"
)

type Command interface {
	Message
	String() string
}

type TTPParams interface{}

type ResultHandler = func(source Entity, args ...any) (Event, error)
type TTP struct {
	ID          string
	Name        string
	Description string
	Tactics     []string
	Technique   []string

	References []string // ms_id::String = ""

	Cmd  string
	Port uint

	Command   Command
	CommandFn func(TTP) Message

	TargetId        string
	Target          string
	TargetNamespace string

	Requires      map[string]string
	Effect        func() string
	ResultHandler ResultHandler
	Params        TTPParams

	// action::Union{String,Function,Nothing} = nothing
	// cmd_args::Union{String,Nothing} = nothing
}

func (ttp TTP) GetTitle() string {
	return ttp.Name
}
func (ttp TTP) GetDescription() string {
	return ttp.Name
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
	SetGroundedString(string)
}

type Targeter interface {
	GetTarget() Target
	SetTarget(Entity)
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

func (t *Target) GetTarget() Target {
	return *t
}

func (t *Target) SetTarget(e Entity) {
	t.Id = e.GetId()
	t.Name = e.GetName()
	t.Entity = e

	if nsEntity, ok := e.(Namespaced); ok {
		t.Ns = nsEntity.GetNamespace()
	}
	// t.Target = newTarget
}

type StartListener struct {
	Port uint
}

func (c StartListener) MessageName() string {
	return "StartListener"
}
func (c StartListener) String() string {
	return fmt.Sprintf("Listener on port %d started", c.Port)
}

type StartC2Redirector struct {
	DstPort uint
}

func (c StartC2Redirector) MessageName() string {
	return "StartC2Redirector"
}
func (c StartC2Redirector) String() string {
	return "Started C2 redirector"
}

type ReadEnvVars struct {
	*Target
}

func (c ReadEnvVars) MessageName() string {
	return "ReadEnvVars"
}
func (c ReadEnvVars) String() string {
	return "Read environment variables"
}

type C2Channel interface{}

type ExecTTP struct {
	TTP  TTP
	Cmd  string
	Args []string
	C2Channel
	*Target
}

func (e *ExecTTP) MessageName() string {
	return "ExecCmd"
}

func (e *ExecTTP) String() string {
	return fmt.Sprintf("Executed '%s' on %s/%s", e.Cmd, e.Target.Ns, e.Target.Name)
}

func (e *ExecTTP) GetTemplate() string {
	return e.Cmd
}

func (e *ExecTTP) SetGroundedString(value string) {
	e.Cmd = value
}

type KubectlExec struct {
}
