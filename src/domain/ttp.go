package domain

import (
	"strings"

	"github.com/creasty/defaults"
)

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

type Parameter struct {
	// Name        string `yaml:"name"`
	Type        string   `yaml:"type"`
	Required    bool     `yaml:"required" default:"true"`
	Description string   `yaml:"description"`
	Examples    []string `yaml:"examples"`
}

func (p *Parameter) UnmarshalYAML(unmarshal func(interface{}) error) error {
	_ = defaults.Set(p)

	type plain Parameter
	if err := unmarshal((*plain)(p)); err != nil {
		return err
	}
	return nil
}

type TTP struct {
	ID          string   `yaml:"id"`
	Name        string   `yaml:"name"`
	Description string   `yaml:"description"`
	Tactic      Tactic   `yaml:"tactic"`
	Technique   []string `yaml:"technique"`

	References []string `yaml:"references"`

	CmdVariants []CmdVariant         `yaml:"cmdVariants"`
	HttpCmd     HttpCmd              `yaml:"httpCmd"`
	Params      map[string]Parameter `yaml:"parameters"`
	Args        map[string]string    `yaml:"args"`
	Port        uint                 `yaml:"port"`

	// Command    string `yaml:"command"`
	CommandMsg Message // during unmarshal converted via Alias to the message

	Execute CodeSnippet `yaml:"execute"`

	Requires Requirements `yaml:"preconditions"`
	Effects  []string     `yaml:"effects"`
	Parser   string       `yaml:"parser"`
	// ParserFn      func(any) any `yaml:"parser"`
	ResultHandler ResultHandler
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

type ToolFunctions []TTP

type Tool struct {
	Name  string        `yaml:"name"`
	TTPs  ToolFunctions `yaml:"functions"`
	Bin   string        `yaml:"bin"`
	Local bool          `yaml:"local"`
}

type YAMLToolFunctions map[string]TTP

// Implements the Unmarshaler interface of the yaml pkg.
func (t *ToolFunctions) UnmarshalYAML(unmarshal func(interface{}) error) error {
	var functions YAMLToolFunctions
	err := unmarshal(&functions)
	if err != nil {
		return err
	}

	ttps := []TTP{}
	for name, ttp := range functions {
		if ttp.Name == "" {
			ttp.Name = strings.Replace(name, "_", " ", -1)
		}
		ttps = append(ttps, ttp)
	}
	*t = ttps
	return nil
}

type ToolFunction struct {
}

var CmdMapping = map[string]Message{
	"StartListener":    StartListener{},
	"CreateRedirector": StartC2Redirector{},
}

type TTPAlias TTP
type YAMLTTP struct {
	TTPAlias `yaml:",inline"` // alias is necessary to avoid infinite loop during Unmarshaling TTP -> YAMLTTP (with embedded TTP)
	// Parser   string           `yaml:"parser"`
	Command string `yaml:"command"`
	// Preconditions map[string]interface{} `yaml:"preconditions"`
}

func (t YAMLTTP) TTP() (TTP, error) {
	ttp := TTP(t.TTPAlias)

	cmd, isMessage := parseCommandToMessage(t.Command)
	if isMessage {
		ttp.CommandMsg = cmd
	} else {
		ttp.CmdVariants = append(ttp.CmdVariants, CmdVariant{
			Key:     "",
			Command: t.Command,
		})

	}
	// ttp.Parser = parsers.HandleSaTokenRead
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

func parseCommandToMessage(cmd string) (Message, bool) {
	if msg, ok := CmdMapping[cmd]; ok {
		return msg, true
	}
	return nil, false
}

func (ttp TTP) HandleResult(source Entity, args ...any) (Event, error) {
	if ttp.ResultHandler == nil {
		return nil, nil
	}
	return ttp.ResultHandler(source, args...)
}

type ParserFn func(source Entity, args ...any) (Event, error)

// func (e *ParserFn) UnmarshalYAML(unmarshal func(interface{}) error) error {
// 	return nil // TODO: explore option of lazy evaluation?
// }
