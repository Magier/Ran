package domain

import (
	"fmt"

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

// GetID implements Command.
func (c CommandImpl) GetID() string {
	return c.ID
}

// IsCommand implements Command.
func (c CommandImpl) IsCommand() {}

func NewCmd() CommandImpl {
	return CommandImpl{ID: uuid.NewString()}
}

type TTPParams interface{}

type ResultHandler = func(source Entity, args ...any) (Event, error)

type CodeSnippet struct {
	Lang       string            `yaml:"lang"`
	Code       string            `yaml:"code"`
	Parameters map[string]string `yaml:"parameters"`
}

type Tactic string

const (
	Reconnaissance      = "TA0043"
	ResourceDevelopment = "TA0042"
	InitialAccess       = "TA0001"
	Execution           = "TA0002"
	Persistence         = "TA0003"
	PrivilegeEscalation = "TA0004"
	DefenseEvasion      = "TA0005"
	CredentialAccess    = "TA0006"
	Discovery           = "TA0007"
	LateralMovement     = "TA0008"
	Collection          = "TA0009"
	CommandAndControl   = "TA0011"
	Exfiltration        = "TA0010"
	Impact              = "TA0040"
)

type HttpCmd struct {
	Endpoint string
	Method   string
	Args     []string
	Headers  map[string]string
	Body     string
}

type CmdVariant struct {
	Key     string
	Command string
}

// func (v CmdVariant) GetCmd() string {
// 	return v.Command
// }

type TTP struct {
	ID          string   `yaml:"id"`
	Name        string   `yaml:"name"`
	Description string   `yaml:"description"`
	Tactics     []Tactic `yaml:"tactics"`
	Technique   []string `yaml:"technique"`

	References []string `yaml:"references"`

	Cmd         string            `yaml:"cmd"`
	CmdVariants []CmdVariant      `yaml:"cmdVariants"`
	HttpCmd     HttpCmd           `yaml:"httpCmd"`
	Args        map[string]string `yaml:"args"`
	Port        uint              `yaml:"port"`

	Command   Command           `yaml:"command"`
	CommandFn func(TTP) Message `yaml:"-"`

	Execute CodeSnippet `yaml:"execute"`

	TargetId        string `yaml:"target_id"`
	Target          string `yaml:"target"`
	TargetNamespace string `yaml:"target_namespace"`

	Requires      Requirements  `yaml:"preconditions"`
	Effects       []string      `yaml:"effects"`
	ResultHandler ResultHandler `yaml:"-"`
	Params        TTPParams     `yaml:"params"`
}

func (ttp TTP) GetID() string {
	return ttp.Name
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
		return ExecTTP{
			CommandImpl: NewCmd(),
			TTP:         ttp, Args: ttp.Args,
		}
	}
}

func (ttp TTP) HandleResult(source Entity, args ...any) (Event, error) {
	if ttp.ResultHandler == nil {
		return nil, nil
	}
	return ttp.ResultHandler(source, args...)
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
	TTP         TTP
	CmdVariants []CmdVariant
	Args        map[string]string
	C2Channel   C2Channel
	Target      Entity
}

func (e ExecTTP) GetTarget() Entity {
	return e.Target
}

func (e ExecTTP) String() string {
	var target string
	if e.C2Channel != nil {
		target = e.C2Channel.GetTargetId()
	} else {
		target = e.Target.GetId()
	}

	return fmt.Sprintf("Executed '%s' on %s", e.GetCommand(""), target)
	// return fmt.Sprintf("Executed '%s' on %s", e.TTP.Name, target)
}

func (e ExecTTP) GetCommand(variant string) string {
	if variant != "" {
		for _, v := range e.CmdVariants {
			if v.Key == variant {
				return v.Command
			}
		}
	}

	if len(e.CmdVariants) > 0 {
		return e.CmdVariants[0].Command
	}
	return ""
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
