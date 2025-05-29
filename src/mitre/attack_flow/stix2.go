package attackflow

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"reflect"
	"strings"
	"time"

	"github.com/google/uuid"
)

const MitreUUID = "fb9c968a-745b-4ade-9b25-c324172197f4"

//go:generate go run gen_mappings.go https://raw.githubusercontent.com/mitre-attack/attack-stix-data/master/enterprise-attack/enterprise-attack.json

type StixBundle struct {
	Type        string      `json:"type"`
	ID          string      `json:"id"`
	SpecVersion string      `json:"spec_version"`
	Created     time.Time   `json:"created"`
	Modified    time.Time   `json:"modified"`
	Objects     ObjectSlice `json:"objects"` // use a map interntally for easier management of objects
}

func NewStixBundle() StixBundle {
	now := time.Now()

	mitreTime := time.Date(2022, time.August, 2, 19, 34, 35, 143000000, time.UTC)
	mitreId := "identity--" + MitreUUID
	mitre := Identity{
		SDO: SDO{
			Type:         "identity",
			ID:           mitreId,
			SpecVersion:  "2.1",
			CreatedByRef: mitreId,
			Created:      Timestamp(mitreTime),
			Modified:     Timestamp(mitreTime),
			Name:         "MITRE Engenuity Center for Threat-Informed Defense",
		},
		IdentityClass: Organization,
	}

	extDef := ExtensionDefinition{
		SDO: SDO{
			Type:         "extension-definition",
			ID:           "extension-definition--" + MitreUUID,
			SpecVersion:  "2.1",
			CreatedByRef: mitre.ID,
			Created:      Timestamp(mitreTime),
			Modified:     Timestamp(mitreTime),
			Name:         "Attack Flow",
			Description:  "Extends STIX 2.1 with features to create Attack Flows.",
			ExternalReferences: []ExternalReference{
				{
					SourceName:  "Documentation",
					Description: "Attack Flow Documentation",
					URL:         "https://center-for-threat-informed-defense.github.io/attack-flow",
				},
				{
					SourceName:  "GitHub",
					Description: "Attack Flow GitHub Repository",
					URL:         "https://github.com/center-for-threat-informed-defense/attack-flow",
				},
			},
		},
		Schema:         "https://center-for-threat-informed-defense.github.io/attack-flow/stix/attack-flow-schema-2.0.0.json",
		Version:        "2.0.0",
		ExtensionTypes: []string{"new-sdo"},
	}

	objSlice := ObjectSlice{
		extDef.ID: extDef,
		mitreId:   mitre,
	}

	return StixBundle{
		Type:        "bundle",
		ID:          fmt.Sprintf("bundle--%s", uuid.New()),
		SpecVersion: "2.1",
		Created:     now,
		Modified:    now,
		Objects:     objSlice,
	}
}

func UnmarshalStixBundle(data []byte) (StixBundle, error) {
	var r StixBundle
	err := json.Unmarshal(data, &r)
	return r, err
}

func (r *StixBundle) Marshal() ([]byte, error) {
	return json.Marshal(r)
}

type ObjectSlice map[string]StixObject

func (o ObjectSlice) Append(objects ...StixObject) ObjectSlice {
	for _, obj := range objects {
		o[obj.GetID()] = obj
	}
	return o
}
func (o ObjectSlice) MarshalJSON() ([]byte, error) {
	objects := make([]StixObject, 0, len(o))
	for _, obj := range o {
		objects = append(objects, obj)
	}
	return json.Marshal(objects)
}

func (objects *ObjectSlice) UnmarshalJSON(data []byte) error {
	var obj []json.RawMessage
	if err := json.Unmarshal(data, &obj); err != nil {
		return err
	}
	*objects = make(ObjectSlice)

	// Create a mapping from SDO types to factory functions.
	// see https://mitre-attack.github.io/attack-data-model/docs/overview
	for _, raw := range obj {
		sdo := SDO{}
		if err := json.Unmarshal(raw, &sdo); err != nil {
			e := err.Error()
			slog.Error(e)
			return err
		}

		var err error
		if factory, ok := StixObjectFactory[sdo.Type]; !ok {
			fmt.Println("Skipping unknown SDO type:", sdo.Type)
			continue
		} else {
			// Create a pointer instance for JSON unmarshalling.
			instancePtr := factory()

			// nil pointer means the type should be skipped
			if instancePtr == nil {
				continue
			}

			err = json.Unmarshal(raw, instancePtr)
			if err == nil {
				// Convert the pointer to a value before appending.
				value := reflect.ValueOf(instancePtr).Elem().Interface()
				if stixObj, ok := value.(StixObject); ok {
					(*objects)[stixObj.GetID()] = stixObj
					// *objects = append(*objects, stixObj)
				} else {
					err = fmt.Errorf("unmarshalled object does not implement StixObject")
				}
			}
		}
		if err != nil {
			slog.Error(err.Error())
			return err
		}
	}

	return nil
}

type MitreCollection struct {
	SDO          `json:",inline"`
	MitreVersion string `json:"x-mitre_version"`
}

type MitreMatrix struct {
	SDO        `json:",inline"`
	TacticRefs []string `json:"tactic_refs"`
}

type KillChainPhase struct {
	KillChainName string `json:"kill_chain_name"`
	PhaseName     string `json:"phase_name"`
}

type AttackPattern struct {
	SDO            `json:",inline"`
	KillChain      []KillChainPhase `json:"kill_chain_phases"`
	Platforms      []string         `json:"x_mitre_platforms"`
	Deprecated     bool             `json:"x_mitre_deprecated"`
	Detection      string           `json:"x_mitre_detection"`
	IsSubtechnique bool             `json:"x_mitre_is_subtechnique"`
}

type MitreTactic struct {
	SDO       `json:",inline"`
	Domains   []string `json:"x_mitre_domains"`
	Shortname string   `json:"x_mitre_shortname"`
}

type Tool struct {
	SDO        `json:",inline"`
	Platforms  []string `json:"x_mitre_platforms"`
	Deprecated bool     `json:"x_mitre_deprecated"`
	Alias      []string `json:"x_mitre_aliases"`
	Domain     []string `json:"x_mitre_domains"`
}

var StixObjectFactory = map[string]func() StixObject{
	"extension-definition":   func() StixObject { return &ExtensionDefinition{} },
	"identity":               func() StixObject { return &Identity{} },
	"attack-flow":            func() StixObject { return &AttackFlow{} },
	"attack-action":          func() StixObject { return &AttackAction{} },
	"attack-condition":       func() StixObject { return &AttackCondition{} },
	"attack-pattern":         func() StixObject { return &AttackPattern{} },
	"attack-asset":           func() StixObject { return &AttackAsset{} },
	"attack-operator":        func() StixObject { return &AttackOperator{} },
	"relationship":           func() StixObject { return &Relationship{} },
	"infrastructure":         func() StixObject { return &Infrastructure{} },
	"process":                func() StixObject { return &Process{} },
	"note":                   func() StixObject { return &Note{} },
	"x-mitre-collection":     func() StixObject { return &MitreCollection{} },
	"x-mitre-tactic":         func() StixObject { return &MitreTactic{} },
	"campaign":               func() StixObject { return nil },
	"course-of-action":       func() StixObject { return nil },
	"intrusion-set":          func() StixObject { return nil },
	"malware":                func() StixObject { return nil },
	"tool":                   func() StixObject { return &Tool{} },
	"x-mitre-data-source":    func() StixObject { return nil }, // TODO: can be a Container, so may be interesting
	"x-mitre-data-component": func() StixObject { return nil },
	"x-mitre-matrix":         func() StixObject { return &MitreMatrix{} },
	"marking-definition":     func() StixObject { return nil },
}

type Timestamp time.Time

const TimestampLayout = "2006-01-02T15:04:05.999Z"

func (ts *Timestamp) UnmarshalJSON(data []byte) error {
	s := strings.Trim(string(data), "\"")
	if s == "null" {
		return fmt.Errorf("No valid timestamp provided in Stixbundle")
	}
	t, err := time.Parse(TimestampLayout, s)
	if err != nil {
		return fmt.Errorf("Failed to parse timestamp: %v", err)
	}
	*ts = Timestamp(t)
	return nil
}
func (t Timestamp) MarshalJSON() ([]byte, error) {
	tt := time.Time(t)
	// STIX timestamp must be  RFC 3339-formatted timestamp using UTC
	// https://docs.oasis-open.org/cti/stix/v2.1/os/stix-v2.1-os.html#_ksbm2nost85y
	return json.Marshal(tt.Format(TimestampLayout))
}

type SDO struct {
	Type               string              `json:"type"`
	ID                 string              `json:"id"`
	SpecVersion        string              `json:"spec_version"`
	CreatedByRef       string              `json:"created_by_ref,omitempty"`
	Created            Timestamp           `json:"created"`
	Modified           Timestamp           `json:"modified"`
	Name               string              `json:"name"`
	Confidence         *int                `json:"confidence,omitempty"`
	Description        string              `json:"description"`
	ExternalReferences []ExternalReference `json:"external_references,omitempty"`
	Extensions         Extensions          `json:"extensions,omitempty"`
}
type Extensions map[string]ExtensionDefInstance

func NewSDO(sdoType, name, description string, isExtension bool) SDO {
	now := time.Now()
	extensions := Extensions{}
	if isExtension {
		extensions["extension-definition--fb9c968a-745b-4ade-9b25-c324172197f4"] = ExtensionDefInstance{
			ExtensionType: "new-sdo",
		}
	}

	return SDO{
		Type:               sdoType,
		ID:                 fmt.Sprintf("%s--%s", sdoType, uuid.New()),
		SpecVersion:        "2.1",
		Created:            Timestamp(now),
		Modified:           Timestamp(now),
		Name:               name,
		Description:        description,
		ExternalReferences: []ExternalReference{},
		Extensions:         extensions,
	}
}

func (sdo SDO) GetID() string {
	return sdo.ID
}
func (sdo SDO) GetType() string {
	return sdo.Type
}

type StixObject interface {
	GetID() string
	GetType() string
}

type ExternalReference struct {
	SourceName  string `json:"source_name"`
	Description string `json:"description"`
	URL         string `json:"url"`
	ExternalID  string `json:"external_id"`
}
type ExtensionDefinition struct {
	SDO            `json:",inline"`
	Schema         string   `json:"schema"`
	Version        string   `json:"version"`
	ExtensionTypes []string `json:"extension_types"`
}

type Extension struct {
	Type string `json:"type"`
	// Properties ExtensionsProperties `json:"properties"`
	Required []string `json:"required"`
}

type IdentityClass string

const (
	Individual   IdentityClass = "Individual"
	Group        IdentityClass = "Group"
	Organization IdentityClass = "Organization"
	System       IdentityClass = "System"
	Class        IdentityClass = "Class"
	Unknown      IdentityClass = "Unknown"
)

type Identity struct {
	SDO                `json:",inline"`
	IdentityClass      IdentityClass `json:"identity_class"`
	ContactInformation string        `json:"contact_information,omitempty"`
}

// Specifies infrastructure used for command and control (C2). This is typically a domain name or IP address.
const InfraTypeC2 = "command-and-control"

// Specific infrastructure used for anonymization, such as a proxy.
const InfraTypeAnonymization = "anonymization"

const InfraTypeExfiltration = "exfiltration"
const InfraTypeStaging = "staging"

// All InfaTypes: https://docs.oasis-open.org/cti/stix/v2.1/cs01/stix-v2.1-cs01.html#_67vrmztjft3h

type Infrastructure struct {
	SDO   `json:",inline"`
	Types []string `json:"infrastructure_types,omitempty"`
}

type Note struct {
	SDO        `json:",inline"`
	Abstract   string   `json:"abstract"`
	Content    string   `json:"content"`
	ObjectRefs []string `json:"object_refs"`
}

// type RelationshipType string

// const (
// 	Indicates IdentityClass = "indicates"
// 	BasedOn   IdentityClass = "based-on"
// )

const RelatedTo = "related-to"

// https://docs.oasis-open.org/cti/stix/v2.1/cs01/stix-v2.1-cs01.html#_cqhkqvhnlgfh

type SRO struct {
	ID          string    `json:"id"`
	Type        string    `json:"type"`
	SpecVersion string    `json:"spec_version"`
	Created     Timestamp `json:"created,omitempty"`
	Modified    Timestamp `json:"modified,omitempty"`
	Description string    `json:"description,omitempty"`
}

func (sro SRO) GetID() string {
	return sro.ID
}
func (sro SRO) GetType() string {
	return sro.Type
}

func NewSRO(scoType string) SRO {
	return SRO{
		ID:          fmt.Sprintf("%s--%s", scoType, uuid.New()),
		Type:        scoType,
		SpecVersion: "2.1",
	}
}

type Relationship struct {
	SRO       `json:",inline"`
	SourceRef string `json:"source_ref"`
	TargetRef string `json:"target_ref"`
	Type      string `json:"relationship_type"`
}

func Newrelationship(srcRef, targetRef, label string) Relationship {
	return Relationship{
		SRO:       NewSRO("relationship"),
		SourceRef: srcRef,
		TargetRef: targetRef,
		Type:      label,
	}
}

type ExtensionDefInstance struct {
	ExtensionType string `json:"extension_type"`
}

type SCO struct {
	Type        string `json:"type"`
	ID          string `json:"id"`
	SpecVersion string `json:"spec_version"`
}

func (sco SCO) GetID() string {
	return sco.ID
}
func (sco SCO) GetType() string {
	return sco.Type
}

func NewSCO(scoType string) SCO {
	return SCO{
		ID:          fmt.Sprintf("%s--%s", scoType, uuid.New()),
		Type:        scoType,
		SpecVersion: "2.1",
	}
}

// Schema: https://docs.oasis-open.org/cti/stix/v2.1/cs01/stix-v2.1-cs01.html#_hpppnm86a1jm
type Process struct {
	SCO         `json:",inline"`
	PID         int               `json:"pid,omitempty"`
	Cwd         string            `json:"cwd,omitempty"`
	CommandLine string            `json:"command_line"`
	CreatedTime Timestamp         `json:"created_time"`
	EnvVars     map[string]string `json:"environment_variables,omitempty"`
	IsHidden    bool              `json:"is_hidden,omitempty"`
	// The list of network connections opened by the process,
	//  as a reference to one or more Network Traffic objects.
	OpenedConnectionRefs []string `json:"opened_connection_refs,omitempty"`
	// The user that created the process, as a reference to a User Account object.
	CreatorUserRef string `json:"creator_user_ref,omitempty"`
	// The executable binary that was executed as the process image, as a reference to a File object.
	ImageRef string `json:"image_ref,omitempty"`
	// The other process that spawned (i.e. is the parent of) this one, as a reference to a Process object.
	ParentRef string `json:"parent_ref,omitempty"`
	// The other processes that were spawned by (i.e. children of) this process,
	//  as a reference to one or more other Process objects.
	ChildRefs []string `json:"child_refs,omitempty"`
}

// Schema: https://docs.oasis-open.org/cti/stix/v2.1/cs01/stix-v2.1-cs01.html#_99bl2dibcztv
type File struct {
	SCO    `json:",inline"`
	Name   string   `json:"name"`
	Hashes []string `json:"hashes"`
	// Specifies a list of references to other Cyber-observable Objects contained within the file,
	//  such as another file that is appended to the end of the file,
	//  or an IP address that is contained somewhere in the file.
	// This is intended for use cases other than those targeted by the Archive extension.
	ContainsRefs []string `json:"contains_refs"`
}
