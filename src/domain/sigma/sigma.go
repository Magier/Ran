package sigma

// Package sigma provides minimal-yet-flexible Go structs and (un)marshalling
// logic for working with Sigma detection rules in both YAML and JSON.

import (
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"maps"
	"sort"
	"strings"

	yaml "gopkg.in/yaml.v3"
)

// Rule represents a Sigma rule. It purposely models common, stable fields.
// https://github.com/SigmaHQ/sigma-specification
type Rule struct {
	Title       string    `json:"title" yaml:"title"`
	ID          string    `json:"id,omitempty" yaml:"id,omitempty"`
	Status      string    `json:"status,omitempty" yaml:"status,omitempty"`
	Description string    `json:"description,omitempty" yaml:"description,omitempty"`
	References  []string  `json:"references,omitempty" yaml:"references,omitempty"`
	Author      string    `json:"author,omitempty" yaml:"author,omitempty"`
	Date        string    `json:"date,omitempty" yaml:"date,omitempty"`
	Modified    string    `json:"modified,omitempty" yaml:"modified,omitempty"`
	Tags        []string  `json:"tags,omitempty" yaml:"tags,omitempty"`
	LogSource   LogSource `json:"logsource,omitempty" yaml:"logsource,omitempty"`
	Detection   Detection `json:"detection" yaml:"detection"`
	// Fields         []string  `json:"fields,omitempty" yaml:"fields,omitempty"`
	FalsePositives []string `json:"falsepositives,omitempty" yaml:"falsepositives,omitempty"`
	Level          string   `json:"level,omitempty" yaml:"level,omitempty"`
}

func (r Rule) ToYAMLString() (string, error) {
	b, err := yaml.Marshal(r)
	if err != nil {
		return "", err
	}
	return string(b), nil
}

// UnmarshalYAML extracts fields, adding unknown ones as tags.
func (r *Rule) UnmarshalYAML(node *yaml.Node) error {
	type raw Rule
	aux := struct {
		*raw `yaml:",inline"`
		Rest map[string]any `yaml:",inline"`
	}{
		raw: (*raw)(r),
	}

	// store any unknown fields as tags formatted <key>.<value>
	if err := node.Decode(&aux); err != nil {
		return err
	}
	for k, v := range aux.Rest {
		r.Tags = append(r.Tags, fmt.Sprintf("%s.%s", k, oneLine(v)))
	}
	return nil
}

// helper function to squash an arbitrary YAML node into a single line
func oneLine(value any) string {
	// Marshal back to YAML for consistent representation (works for scalars, maps, slices)
	b, _ := yaml.Marshal(value)
	s := strings.TrimSpace(string(b))
	s = strings.ReplaceAll(s, "\n", " ")
	s = strings.Join(strings.Fields(s), " ")
	return s
	// if value.Kind == yaml.ScalarNode {
	// 	return strings.TrimSpace(value.Value)
	// }
	// var v any
	// if err := value.Decode(&v); err != nil {
	// 	return "" // or handle error
	// }
	// b, _ := yaml.Marshal(v)
	// s := strings.TrimSpace(string(b))
	// s = strings.Join(strings.Fields(s), " ")
	// return s
}

type LogSource struct {
	Product  string `json:"product,omitempty" yaml:"product,omitempty"`
	Service  string `json:"service,omitempty" yaml:"service,omitempty"`
	Category string `json:"category,omitempty" yaml:"category,omitempty"`
}

// Detection holds the dynamic detection selections and fixed keys like
// condition and timeframe. All user-defined selection blocks (e.g. "selection1",
// "sel_cmdline", "filter_main", etc.) live in Selections.
//
// Values under Selections are intentionally typed as any to support nested
// maps/arrays/strings/bools/numbers exactly as authored in Sigma.

type Detection struct {
	// Arbitrary selection/filter identifiers => selection bodies
	Selections map[string]any `json:"-" yaml:"-"`

	// Fixed Sigma keys inside detection
	Condition string `json:"condition" yaml:"condition"`
	Timeframe string `json:"timeframe,omitempty" yaml:"timeframe,omitempty"`
}

// NewDetection creates a Detection with an initialized Selections map.
func NewDetection() Detection {
	return Detection{Selections: make(map[string]any)}
}

// SetSelection is a convenience for adding or replacing a selection block.
func (d *Detection) SetSelection(name string, body any) {
	if d.Selections == nil {
		d.Selections = make(map[string]any)
	}
	d.Selections[name] = body
}

// GetSelection retrieves a selection block by name.
func (d *Detection) GetSelection(name string) (any, bool) {
	if d.Selections == nil {
		return nil, false
	}
	v, ok := d.Selections[name]
	return v, ok
}

// MarshalJSON flattens Selections together with fixed keys for JSON output.
func (d Detection) MarshalJSON() ([]byte, error) {
	slog.Info("Marshaling Detection to JSON", "selections", d.Selections, "condition", d.Condition, "timeframe", d.Timeframe)
	m := make(map[string]any, len(d.Selections)+2)
	maps.Copy(m, d.Selections)
	if d.Timeframe != "" {
		m["timeframe"] = d.Timeframe
	}
	m["condition"] = d.Condition
	return json.Marshal(m)
}

// UnmarshalJSON extracts fixed keys, placing the rest into Selections.
func (d *Detection) UnmarshalJSON(b []byte) error {
	var m map[string]any
	if err := json.Unmarshal(b, &m); err != nil {
		return err
	}
	if d.Selections == nil {
		d.Selections = make(map[string]any)
	}
	for k, v := range m {
		switch k {
		case "condition":
			str, ok := v.(string)
			if !ok {
				return fmt.Errorf("detection.condition must be string, got %T", v)
			}
			d.Condition = str
		case "timeframe":
			if v == nil {
				continue
			}
			str, ok := v.(string)
			if !ok {
				return fmt.Errorf("detection.timeframe must be string, got %T", v)
			}
			d.Timeframe = str
		default:
			d.Selections[k] = v
		}
	}
	if d.Condition == "" {
		return errors.New("detection.condition is required")
	}
	return nil
}

// MarshalYAML mirrors MarshalJSON, flattening Selections and adding fixed keys.
func (d Detection) MarshalYAML() (any, error) {
	// Ensure stable key order in YAML output by sorting keys
	keys := make([]string, 0, len(d.Selections))
	for k := range d.Selections {
		keys = append(keys, k)
	}
	sort.Strings(keys)

	m := make(map[string]any, len(keys)+2)
	for _, k := range keys {
		m[k] = d.Selections[k]
	}
	if d.Timeframe != "" {
		m["timeframe"] = d.Timeframe
	}
	m["condition"] = d.Condition
	return m, nil
}

// UnmarshalYAML extracts fixed keys, placing the rest into Selections.
func (d *Detection) UnmarshalYAML(node *yaml.Node) error {
	var m map[string]any
	if err := node.Decode(&m); err != nil {
		return err
	}
	if d.Selections == nil {
		d.Selections = make(map[string]any)
	}
	for k, v := range m {
		switch k {
		case "condition":
			str, ok := v.(string)
			if !ok {
				return fmt.Errorf("detection.condition must be string, got %T", v)
			}
			d.Condition = str
		case "timeframe":
			if v == nil {
				continue
			}
			str, ok := v.(string)
			if !ok {
				return fmt.Errorf("detection.timeframe must be string, got %T", v)
			}
			d.Timeframe = str
		default:
			d.Selections[k] = v
		}
	}
	if d.Condition == "" {
		return errors.New("detection.condition is required")
	}
	return nil
}

// ExampleJSON demonstrates marshaling a Rule to JSON.
//
// func ExampleJSON() {
// 	r := Rule{
// 		Title:   "Suspicious PowerShell",
// 		ID:      "123e4567-e89b-12d3-a456-426614174000",
// 		Status:  "experimental",
// 		Author:  "Your Name",
// 		Date:    "2025/08/28",
// 		Tags:    []string{"attack.execution", "powershell"},
// 		LogSource: LogSource{Product: "windows", Service: "sysmon"},
// 		Detection: func() Detection {
// 			d := NewDetection()
// 			d.SetSelection("selection_cmd", map[string]any{
// 				"CommandLine|contains": []any{"-EncodedCommand", "-enc"},
// 			})
// 			d.SetSelection("filter_known_good", map[string]any{
// 				"Image|endswith": "\\\\powershell.exe",
// 			})
// 			d.Condition = "selection_cmd and not filter_known_good"
// 			return d
// 		}(),
// 		Level: "high",
// 	}
//
// 	b, _ := json.MarshalIndent(r, "", "  ")
// 	fmt.Println(string(b))
// }
//
// ExampleYAML demonstrates marshaling a Rule to YAML.
//
// func ExampleYAML() {
// 	r := Rule{ /* ... build rule as above ... */ }
// 	b, _ := yaml.Marshal(r)
// 	fmt.Println(string(b))
// }

// Validate performs basic sanity checks on a Rule.
// Extend with your project-specific invariants as needed.
func (r Rule) Validate() error {
	if r.Title == "" {
		return errors.New("title is required")
	}
	if err := r.Detection.validate(); err != nil {
		return err
	}
	return nil
}

func (d Detection) validate() error {
	if d.Condition == "" {
		return errors.New("detection.condition is required")
	}
	return nil
}

// MarshalRuleJSON is a convenience wrapper returning pretty JSON.
func MarshalRuleJSON(r Rule, indent bool) ([]byte, error) {
	if indent {
		return json.MarshalIndent(r, "", "  ")
	}
	return json.Marshal(r)
}

// UnmarshalRuleJSON parses JSON bytes into a Rule.
func UnmarshalRuleJSON(b []byte) (Rule, error) {
	var r Rule
	if err := json.Unmarshal(b, &r); err != nil {
		return Rule{}, err
	}
	return r, nil
}

// MarshalRuleYAML returns YAML bytes.
func MarshalRuleYAML(r Rule) ([]byte, error) {
	return yaml.Marshal(r)
}

// UnmarshalRuleYAML parses YAML bytes into a Rule.
func UnmarshalRuleYAML(b []byte) (Rule, error) {
	var r Rule
	if err := yaml.Unmarshal(b, &r); err != nil {
		return Rule{}, err
	}
	return r, nil
}
