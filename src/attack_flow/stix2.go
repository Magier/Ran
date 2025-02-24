package attackflow

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"time"

	"github.com/google/uuid"
)

const MitreUUID = "fb9c968a-745b-4ade-9b25-c324172197f4"

type StixBundle struct {
	Type        string      `json:"type"`
	ID          string      `json:"id"`
	SpecVersion string      `json:"spec_version"`
	Created     time.Time   `json:"created"`
	Modified    time.Time   `json:"modified"`
	Objects     ObjectSlice `json:"objects"`
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

	return StixBundle{
		Type:        "bundle",
		ID:          fmt.Sprintf("bundle--%s", uuid.New()),
		SpecVersion: "2.1",
		Created:     now,
		Modified:    now,
		Objects:     ObjectSlice{extDef, mitre},
	}
}

func UnmarshalAttackFlow(data []byte) (StixBundle, error) {
	var r StixBundle
	err := json.Unmarshal(data, &r)
	return r, err
}

func (r *StixBundle) Marshal() ([]byte, error) {
	return json.Marshal(r)
}

type ObjectSlice []StixObject

func (objects *ObjectSlice) UnmarshalJSON(data []byte) error {
	var obj []json.RawMessage
	if err := json.Unmarshal(data, &obj); err != nil {
		return err
	}

	for _, raw := range obj {
		sdo := SDO{}
		if err := json.Unmarshal(raw, &sdo); err != nil {
			e := err.Error()
			slog.Error(e)
			return err
		}

		// Create a mapping from SDO types to factory functions.
		factories := map[string]func() StixObject{
			"extension-definition": func() StixObject { return &ExtensionDefinition{} },
			"identity":             func() StixObject { return &Identity{} },
			"attack-flow":          func() StixObject { return &AttackFlow{} },
			"attack-action":        func() StixObject { return &AttackAction{} },
			"attack-condition":     func() StixObject { return &AttackCondition{} },
			"attack-asset":         func() StixObject { return &AttackAsset{} },
			"attack-operator":      func() StixObject { return &AttackOperator{} },
			"relationship":         func() StixObject { return &Relationship{} },
			"infrastructure":       func() StixObject { return &Infrastructure{} },
			"note":                 func() StixObject { return &Note{} },
		}

		var err error
		if factory, ok := factories[sdo.Type]; !ok {
			err = fmt.Errorf("%s SDO type is not correctly unmarshalled", sdo.Type)
		} else {
			instance := factory()
			err = json.Unmarshal(raw, instance)
			if err == nil {
				*objects = append(*objects, instance)
			}
		}
		if err != nil {
			slog.Error(err.Error())
			return err
		}
	}

	return nil
}

type Timestamp time.Time

func (t Timestamp) MarshalJSON() ([]byte, error) {
	tt := time.Time(t)
	// STIX timestamp must be  RFC 3339-formatted timestamp using UTC
	// https://docs.oasis-open.org/cti/stix/v2.1/os/stix-v2.1-os.html#_ksbm2nost85y
	return json.Marshal(tt.Format("2006-01-02T15:04:05.000Z"))
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
type Infrastructure struct {
	SDO                 `json:",inline"`
	InfrastructureTypes []string `json:"infrastructure_types,omitempty"`
}
type Note struct {
	SDO        `json:",inline"`
	Abstract   string   `json:"abstract"`
	Content    string   `json:"content"`
	ObjectRefs []string `json:"object_refs"`
}
type Relationship struct {
	SDO
	SourceRef        string `json:"source_ref"`
	TargetRef        string `json:"target_ref"`
	RelationshipType string `json:"relationship_type"`
}

type ExtensionDefInstance struct {
	ExtensionType string `json:"extension_type"`
}
