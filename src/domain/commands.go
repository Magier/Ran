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
	Reconnaissance      = "Reconnaissance"       // TA0043
	ResourceDevelopment = "Resource Development" // TA0042
	InitialAccess       = "Initial Access"       // TA0001
	Execution           = "Execution"            // TA0002
	Persistence         = "Persistence"          // TA0003
	PrivilegeEscalation = "Privilege Escalation" // TA0004
	DefenseEvasion      = "Defense Evasion"      // TA0005
	CredentialAccess    = "Credential Access"    // TA0006
	Discovery           = "Discovery"            // TA0007
	LateralMovement     = "Lateral Movement"     // TA0008
	Collection          = "Collection"           // TA0009
	CommandAndControl   = "Command And Control"  // TA0011
	Exfiltration        = "Exfiltration"         // TA0010
	Impact              = "Impact"               // TA0040
)

type HttpCmd struct {
	Endpoint string
	Method   string
	Args     []string
	Headers  map[string]string
	Body     string
}

type CmdVariant struct {
	Key     string `yaml:"key"`
	Command string `yaml:"command"`
}

// func (v CmdVariant) GetCmd() string {
// 	return v.Command
// }

type ParserFn func(any) any

func (e *ParserFn) UnmarshalYAML(unmarshal func(interface{}) error) error {
	return nil // TODO: explore option of lazy evaluation?
}

type TTP struct {
	ID          string   `yaml:"id"`
	Name        string   `yaml:"name"`
	Description string   `yaml:"description"`
	Tactic      Tactic   `yaml:"tactic"`
	Technique   []string `yaml:"technique"`

	References []string `yaml:"references"`

	CmdVariants []CmdVariant      `yaml:"cmdVariants"`
	HttpCmd     HttpCmd           `yaml:"httpCmd"`
	Args        map[string]string `yaml:"args"`
	Port        uint              `yaml:"port"`

	// Command    string `yaml:"command"`
	CommandMsg Message // during unmarshal converted via Alias to the message

	Execute CodeSnippet `yaml:"execute"`

	Requires Requirements `yaml:"preconditions"`
	Effects  []string     `yaml:"effects"`
	// Parser        string       `yaml:"parser"`
	ParserFn func(any) any
	// ParserFn      func(any) any `yaml:"parser"`
	ResultHandler ResultHandler
	Params        TTPParams `yaml:"params"`
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
	if ttp.CommandMsg != nil {
		return ttp.CommandMsg
	} else {
		return ExecTTP{
			CommandImpl: NewCmd(),
			TTP:         ttp, Args: ttp.Args,
		}
	}
}

var CmdMapping = map[string]Message{
	"StartListener":    StartListener{},
	"CreateRedirector": StartC2Redirector{},
}

type TTPAlias TTP
type YAMLTTP struct {
	TTPAlias `yaml:",inline"` // alias is necessary to avoid infinite loop during Unmarshaling TTP -> YAMLTTP (with embedded TTP)
	Parser   string           `yaml:"parser"`
	Command  string           `yaml:"command"`
	// Preconditions map[string]interface{} `yaml:"preconditions"`
}

func (t YAMLTTP) TTP() (TTP, error) {
	ttp := TTP(t.TTPAlias)

	ttp.CommandMsg = parseCommandToMessage(t.Command)
	return ttp, nil
}

func (ttp *TTP) UnmarshalYAML(unmarshal func(interface{}) error) error {
	// Technique from: https://blog.gopheracademy.com/advent-2016/advanced-encoding-decoding/
	var raw YAMLTTP
	if err := unmarshal(&raw); err != nil {
		return err
	}
	var err error
	*ttp, err = raw.TTP()
	if err != nil {
		return err
	}

	return nil
}

func parseCommandToMessage(cmd string) Message {
	if msg, ok := CmdMapping[cmd]; ok {
		return msg
	}
	return nil
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

	return fmt.Sprintf("Executing '%s' on %s", e.GetCommand(""), target)
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
