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

type CodeSnippet struct {
	Lang       string            `yaml:"lang"`
	Code       string            `yaml:"code"`
	Parameters map[string]string `yaml:"parameters"`
}

type TTP struct {
	ID          string   `yaml:"id"`
	Name        string   `yaml:"name"`
	Description string   `yaml:"description"`
	Tactics     []string `yaml:"tactics"`
	Technique   []string `yaml:"technique"`

	References []string `yaml:"references"`

	Cmd  string   `yaml:"cmd"`
	Args []string `yaml:"args"`
	Port uint     `yaml:"port"`

	Command   Command           `yaml:"command"`
	CommandFn func(TTP) Message `yaml:"-"`

	Execute CodeSnippet `yaml:"execute"`

	TargetId        string `yaml:"target_id"`
	Target          string `yaml:"target"`
	TargetNamespace string `yaml:"target_namespace"`

	Requires      Requirements  `yaml:"requires"`
	Effect        func() string `yaml:"-"`
	Effects       []string      `yaml:"effects"`
	ResultHandler ResultHandler `yaml:"-"`
	Params        TTPParams     `yaml:"params"`
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
	} else if ttp.Command != nil {
		return ttp.Command
	} else {
		return ExecTTP{TTP: ttp, Cmd: ttp.Cmd, Args: ttp.Args}
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
