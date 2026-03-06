package domain

import (
	"fmt"
	"log/slog"
	"strings"

	"github.com/Magier/Ran/domain/sigma"
	"github.com/Magier/Ran/mitre"
	"github.com/creasty/defaults"
	"gopkg.in/yaml.v3"
)

type ResultHandler = func(source Entity, args ...any) (Event, error)

type CodeSnippet struct {
	Lang       string            `yaml:"lang"`
	Code       string            `yaml:"code"`
	Parameters map[string]string `yaml:"parameters"`
	EnvVars    []string          `yaml:"envVars"`
}

type HttpCmd struct {
	Endpoint string
	Method   string
	Args     []string
	Headers  map[string]string
	Body     string
}

type Procedure struct {
	Key            string      `yaml:"key"`
	Command        string      `yaml:"command"`
	Tool           string      `yaml:"tool"`
	IsLocalCommand bool        `yaml:"isLocal"`
	Execute        CodeSnippet `yaml:"execute"`
	Cleanup        CodeSnippet `yaml:"cleanup"`
	Parser         string      `yaml:"parser"` // optional name of a parser to process the output of this procedure
}

func (p Procedure) GetTool() string {
	if p.Tool != "" {
		return p.Tool
	}
	return p.Key
}

// Copy creates a deep copy of the Procedure
func (p Procedure) Copy() Procedure {
	copy := p
	// CodeSnippet fields are copied by value (strings and maps are already value types in this context)
	// The Parameters map in Execute needs to be deep copied
	if p.Execute.Parameters != nil {
		copy.Execute.Parameters = make(map[string]string, len(p.Execute.Parameters))
		for k, v := range p.Execute.Parameters {
			copy.Execute.Parameters[k] = v
		}
	}
	if p.Execute.EnvVars != nil {
		copy.Execute.EnvVars = make([]string, len(p.Execute.EnvVars))
		copy.Execute.EnvVars = append([]string{}, p.Execute.EnvVars...)
	}
	if p.Cleanup.Parameters != nil {
		copy.Cleanup.Parameters = make(map[string]string, len(p.Cleanup.Parameters))
		for k, v := range p.Cleanup.Parameters {
			copy.Cleanup.Parameters[k] = v
		}
	}
	if p.Cleanup.EnvVars != nil {
		copy.Cleanup.EnvVars = make([]string, len(p.Cleanup.EnvVars))
		copy.Cleanup.EnvVars = append([]string{}, p.Cleanup.EnvVars...)
	}
	return copy
}

func (c *Procedure) UnmarshalYAML(unmarshal func(interface{}) error) error {
	type tmpVVariant Procedure
	if err := unmarshal((*tmpVVariant)(c)); err != nil {
		return err
	}
	// go-yaml doesn't properly parse fold-style multiline strings:
	// https://github.com/go-yaml/yaml/issues/789
	// so manually replace the newline characters
	c.Command = strings.ReplaceAll(c.Command, "\n", "")
	return nil
}

type Parameter struct {
	Name        string `yaml:"name"`
	Type        string `yaml:"type"`
	Required    bool   `yaml:"required" default:"true"`
	Description string `yaml:"description"`
	Default     string `yaml:"default"`
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
	ID          string       `yaml:"id" json:"id"`
	Name        string       `yaml:"name" json:"name"`
	Description string       `yaml:"description" json:"description" `
	Tactic      mitre.Tactic `yaml:"tactic" json:"tactic"`
	Techniques  []string     `yaml:"techniques" json:"techniques"`
	Status      string       `yaml:"status" json:"status"` // e.g. "draft", "stable", "deprecated", "disabled"

	References []string `yaml:"references" json:"references"`

	Procedures []Procedure `yaml:"procedures" json:"procedures"`
	// HttpCmd    HttpCmd     `yaml:"httpCmd" json:"httpCmd"`
	Params     []Parameter `json:"params"`
	CommandMsg Message     // during unmarshal converted via Alias to the message

	Requires      Requirements  `yaml:"preconditions" json:"requires"`
	Effects       []string      `yaml:"effects" json:"effects"`
	ResultHandler ResultHandler `json:"-" yaml:"-"`
	Cleanup       Procedure     `yaml:"cleanup" json:"cleanup"`
	Defense       Defense       `yaml:"defense" json:"defense"`
}

func (ttp TTP) GetID() string {
	return ttp.ID
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
			CommandImpl: NewCmd(""),
			// TTP:         ttp, Args: ttp.Args,
		}
	}
}

// Copy creates a deep copy of the TTP to ensure immutability
func (ttp TTP) Copy() TTP {
	copy := ttp

	// Deep copy slices
	if ttp.Techniques != nil {
		copy.Techniques = make([]string, len(ttp.Techniques))
		copy.Techniques = append([]string{}, ttp.Techniques...)
	}

	if ttp.References != nil {
		copy.References = make([]string, len(ttp.References))
		copy.References = append([]string{}, ttp.References...)
	}

	if ttp.Procedures != nil {
		copy.Procedures = make([]Procedure, len(ttp.Procedures))
		for i, proc := range ttp.Procedures {
			copy.Procedures[i] = proc.Copy()
		}
	}

	if ttp.Params != nil {
		copy.Params = make([]Parameter, len(ttp.Params))
		copy.Params = append([]Parameter{}, ttp.Params...)
	}

	if ttp.Effects != nil {
		copy.Effects = make([]string, len(ttp.Effects))
		copy.Effects = append([]string{}, ttp.Effects...)
	}

	// Deep copy Cleanup procedure
	copy.Cleanup = ttp.Cleanup.Copy()

	// Note: CommandMsg and ResultHandler are intentionally not deep copied
	// as they contain function pointers that should be shared

	return copy
}

type Defense struct {
	ID          string      `yaml:"id" json:"id"`
	Name        string      `yaml:"name" json:"name"`
	URL         string      `yaml:"url" json:"url"`
	Description string      `yaml:"description" json:"description"`
	Sigma       *sigma.Rule `yaml:"sigma,omitempty" json:"sigma,omitempty"`
	D3fend      string      `yaml:"d3f" json:"d3f"`
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
		if ttp.ID == "" {
			ttp.ID = ttp.Name
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
	"StopListener":     StopListener{},
	"CreateRedirector": StartC2Redirector{},
}

// whacky hack to keep order of params defined as dict: https://github.com/go-yaml/yaml/issues/698
type ParamSlice []Parameter

func (p *ParamSlice) UnmarshalYAML(value *yaml.Node) error {
	for i := 0; i < len(value.Content); i += 2 {
		var param Parameter
		if err := value.Content[i+1].Decode(&param); err != nil {
			return err
		}
		param.Name = value.Content[i].Value
		*p = append(*p, param)
	}

	return nil
}

type TTPAlias TTP
type YAMLTTP struct {
	TTPAlias   `yaml:",inline"` // alias is necessary to avoid infinite loop during Unmarshaling TTP -> YAMLTTP (with embedded TTP)
	Command    string           `yaml:"command"`
	Parameters ParamSlice       `yaml:"parameters"`
	// Preconditions map[string]interface{} `yaml:"preconditions"`
}

func (t YAMLTTP) TTP() (TTP, error) {
	ttp := TTP(t.TTPAlias)

	// normalize techniques to use the ID
	for i, entry := range ttp.Techniques {
		if !mitre.IsTechniqueID(entry) {
			if id, ok := mitre.GetTechniqueIDByName(entry); ok {
				ttp.Techniques[i] = id
			} else {
				slog.Warn(fmt.Sprintf("TTP '%s' has no valid Mitre technique assigned (value: '%s')", ttp.Name, entry))
			}
		}
	}

	if t.Command != "" {
		cmd, isMessage := parseCommandToMessage(t.Command)
		if isMessage {
			ttp.CommandMsg = cmd
		} else {
			ttp.Procedures = append(ttp.Procedures, Procedure{
				Key:     "",
				Command: t.Command,
			})
		}
	}

	for _, param := range t.Parameters {
		ttp.Params = append(ttp.Params, param)
	}
	if ttp.ID == "" {
		ttp.ID = convertNameToID(ttp.Name)
	}

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

func convertNameToID(name string) string {
	// special characters and emojis are not allowed in TTP IDs
	name = strings.Map(func(r rune) rune {
		if r > 127 {
			return -1
		}
		return r
	}, name)
	name = strings.TrimSpace(name)
	name = strings.ReplaceAll(name, " ", "-")
	name = strings.ReplaceAll(name, "_", "-")
	name = strings.ReplaceAll(name, "/", "-")
	name = strings.ReplaceAll(name, ".", "-")
	name = strings.Map(func(r rune) rune {
		if r == '(' || r == ')' {
			return -1
		}
		return r
	}, name)
	name = strings.ToLower(name)
	return name
}
