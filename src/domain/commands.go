package domain

import (
	"fmt"
)

type Command interface {
	Message
	String() string
}
type TTP interface {
	GetTitle() string
	GetDescription() string
	GetMessage() Message
	HandleResult(Entity, ...any) (Event, error)
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
