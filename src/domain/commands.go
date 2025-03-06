package domain

import (
	"fmt"
	"log/slog"

	"github.com/google/uuid"
)

type Command interface {
	Message
	IsCommand()
	GetID() string
}
type CommandImpl struct {
	ID string
}

func (c *CommandImpl) SetID(id string) {
	c.ID = id
}

// GetID implements Command.
func (c CommandImpl) GetID() string {
	return c.ID
}

// IsCommand implements Command.
func (c CommandImpl) IsCommand() {}

func NewCmd() CommandImpl {
	return CommandImpl{ID: uuid.NewString()}
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

var _ Command = (*StartListener)(nil)

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

type ExecTTP struct {
	CommandImpl
	TTP        TTP
	Variant    CmdVariant
	Args       map[string]string
	C2Channel  C2Channel
	Target     Entity
	CommandMsg Command
}

func (e ExecTTP) GetTarget() Entity {
	return e.Target
}

func (e ExecTTP) String() string {
	var target string
	if e.C2Channel != nil {
		target = e.C2Channel.GetTargetId()
	} else if e.Target != nil {
		target = e.Target.GetId()
	} else {
		slog.Error(fmt.Sprintf("Could not find target for ExecTTP '%s'", e.GetID()))
	}

	return fmt.Sprintf("Executing '%s' on %s", e.TTP.Name, target)
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

type SaveAttackFlow struct {
	CommandImpl
	Path string
}

func (p SaveAttackFlow) String() string {
	return "saveAttackFlow"
}
