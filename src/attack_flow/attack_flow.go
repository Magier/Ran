// This file was generated from JSON Schema using quicktype, do not modify it directly.
// To parse and unparse this JSON data, add this code to your project and do:
//
//    attackFlow, err := UnmarshalAttackFlow(bytes)
//    bytes, err = attackFlow.Marshal()

package attackflow

import (
	"encoding/json"
	"time"
)

func UnmarshalAttackFlow(data []byte) (StixBundle, error) {
	var r StixBundle
	err := json.Unmarshal(data, &r)
	return r, err
}

func (r *StixBundle) Marshal() ([]byte, error) {
	return json.Marshal(r)
}

type StixBundle struct {
	Type        string       `json:"type"`
	ID          string       `json:"id"`
	SpecVersion string       `json:"spec_version"`
	Created     time.Time    `json:"created"`
	Modified    time.Time    `json:"modified"`
	Objects     []StixObject `json:"objects"`
}

type SDO struct {
	Type               string              `json:"type"`
	ID                 string              `json:"id"`
	SpecVersion        string              `json:"spec_version"`
	CreatedByRef       *string             `json:"created-by-ref"`
	Created            time.Time           `json:"created"`
	Modified           time.Time           `json:"modified"`
	Name               string              `json:"name"`
	Confidence         *int                `json:"confidence"`
	Description        string              `json:"description"`
	ExternalReferences []ExternalReference `json:"external_references"`
	Extensions         []Extension         `json:"extensions"`
}

func (sdo SDO) GetType() string {
	return sdo.Type
}

type StixObject interface {
	GetType() string
}

type ExternalReference struct {
	SourceName  string `json:"source_name"`
	Description string `json:"description"`
	URL         string `json:"url"`
}
type ExtensionDefinition struct {
	// ExtensionType string `json:"extension_type"`
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

type AttackFlow struct {
	SDO       `json:",inline"`
	StartRefs []string `json:"start_refs"`
	Scope     string   `json:"scope"`
}

var _ StixObject = (*AttackFlow)(nil)

type Identity struct {
	SDO                `json:",inline"`
	ContactInformation string `json:"contact_information"`
}

type AttackCondition struct {
	SDO            `json:",inline"`
	OnTrueRefs     []string `json:"on_true_refs"`
	OnFalseRefs    []string `json:"on_false_refs"`
	Pattern        string   `json:"pattern"`
	PatternType    string   `json:"pattern_type"`
	PatternVersion string   `json:"pattern_version"`
}

var _ StixObject = (*AttackCondition)(nil)

type AttackAction struct {
	SDO          `json:",inline"`
	TacticID     string   `json:"tactic_id"`
	TacticRef    string   `json:"tactic_ref"`
	TechniqueID  string   `json:"technique_id"`
	TechniqueRef string   `json:"technique_ref"`
	EffectRefs   []string `json:"effect_refs"`
}

var _ StixObject = (*AttackAction)(nil)

type AttackAsset struct {
	SDO       `json:",inline"`
	ObjectRef string `json:"object_ref"`
}

type RelationShip struct {
	SDO
	SourceRef        string `json:"source_ref"`
	TargetRef        string `json:"target_ref"`
	RelationshipType string `json:"relationship_type"`
}

// type Extensions struct {
// 	ExtensionDefinition ExtensionDefinition `json:"extension-definition-"`
// }

type Objects struct {
	Type               string              `json:"type"`
	ID                 string              `json:"id"`
	SpecVersion        string              `json:"spec_version"`
	Created            time.Time           `json:"created"`
	Modified           time.Time           `json:"modified"`
	Name               string              `json:"name,omitempty"`
	Description        string              `json:"description,omitempty"`
	CreatedByRef       string              `json:"created_by_ref,omitempty"`
	Schema             string              `json:"schema,omitempty"`
	Version            string              `json:"version,omitempty"`
	ExtensionTypes     []string            `json:"extension_types,omitempty"`
	ExternalReferences []ExternalReference `json:"external_references,omitempty"`
	IdentityClass      string              `json:"identity_class,omitempty"`
	// Extensions          Extensions          `json:"extensions,omitempty"`
	StartRefs           []string `json:"start_refs,omitempty"`
	Scope               string   `json:"scope,omitempty"`
	ContactInformation  string   `json:"contact_information,omitempty"`
	OnTrueRefs          []string `json:"on_true_refs,omitempty"`
	TacticID            string   `json:"tactic_id,omitempty"`
	TacticRef           string   `json:"tactic_ref,omitempty"`
	TechniqueID         string   `json:"technique_id,omitempty"`
	TechniqueRef        string   `json:"technique_ref,omitempty"`
	EffectRefs          []string `json:"effect_refs,omitempty"`
	Operator            string   `json:"operator,omitempty"`
	InfrastructureTypes []string `json:"infrastructure_types,omitempty"`
	Abstract            string   `json:"abstract,omitempty"`
	Content             string   `json:"content,omitempty"`
	ObjectRefs          []string `json:"object_refs,omitempty"`
	RelationshipType    string   `json:"relationship_type,omitempty"`
	SourceRef           string   `json:"source_ref,omitempty"`
	TargetRef           string   `json:"target_ref,omitempty"`
}

// type AttackFlowAllOf struct {
// 	Comment    string           `json:"$comment"`
// 	Ref        *string          `json:"$ref,omitempty"`
// 	Type       *string          `json:"type,omitempty"`
// 	Properties *AllOfProperties `json:"properties,omitempty"`
// 	Required   []string         `json:"required,omitempty"`
// 	// If         *If              `json:"if,omitempty"`
// 	// Then       *Then            `json:"then,omitempty"`
// 	// Else       *AllOfElse       `json:"else,omitempty"`
// }

// type If struct {
// 	Type       string       `json:"type"`
// 	Properties IfProperties `json:"properties"`
// }

// type IfProperties struct {
// 	Type TypeClass `json:"type"`
// }

// type TypeClass struct {
// 	Type  TypeEnum `json:"type"`
// 	Const string   `json:"const"`
// }

// type Then struct {
// 	Ref string `json:"$ref"`
// }

// type AllOfProperties struct {
// 	Extensions Extensions `json:"extensions"`
// }

// type ExtensionsProperties struct {
// 	ExtensionDefinitionFb9C968A745B4Ade9B25C324172197F4 ExtensionDefinitionFb9C968A745B4Ade9B25C324172197F4 `json:"extension-definition--fb9c968a-745b-4ade-9b25-c324172197f4"`
// }

// type ExtensionDefinitionFb9C968A745B4Ade9B25C324172197F4 struct {
// 	Type       string                                                        `json:"type"`
// 	Properties ExtensionDefinitionFb9C968A745B4Ade9B25C324172197F4Properties `json:"properties"`
// 	Required   []string                                                      `json:"required"`
// }

// type ExtensionDefinitionFb9C968A745B4Ade9B25C324172197F4Properties struct {
// 	ExtensionType TypeClass `json:"extension_type"`
// }

// type Defs struct {
// 	AttackFlow      AttackFlowClass `json:"attack-flow"`
// 	AttackAction    AttackAction    `json:"attack-action"`
// 	AttackAsset     AttackAsset     `json:"attack-asset"`
// 	AttackCondition AttackCondition `json:"attack-condition"`
// 	AttackOperator  AttackOperator  `json:"attack-operator"`
// }

// type AttackAction struct {
// 	Description    string                 `json:"description"`
// 	Type           string                 `json:"type"`
// 	Properties     AttackActionProperties `json:"properties"`
// 	Required       []string               `json:"required"`
// 	XExampleObject string                 `json:"x-exampleObject"`
// }

// type AttackActionProperties struct {
// 	Type           SpecVersion  `json:"type"`
// 	SpecVersion    SpecVersion  `json:"spec_version"`
// 	Name           Description  `json:"name"`
// 	TacticID       Description  `json:"tactic_id"`
// 	TacticRef      TacticRef    `json:"tactic_ref"`
// 	TechniqueID    Description  `json:"technique_id"`
// 	TechniqueRef   Ref          `json:"technique_ref"`
// 	Description    Description  `json:"description"`
// 	ExecutionStart ExecutionEnd `json:"execution_start"`
// 	ExecutionEnd   ExecutionEnd `json:"execution_end"`
// 	CommandRef     Ref          `json:"command_ref"`
// 	AssetRefs      Refs         `json:"asset_refs"`
// 	EffectRefs     Refs         `json:"effect_refs"`
// }

// type Refs struct {
// 	Description string `json:"description"`
// 	Type        string `json:"type"`
// 	Items       Items  `json:"items"`
// 	MinItems    int64  `json:"minItems"`
// }

// type Items struct {
// 	AllOf []ItemsAllOf `json:"allOf"`
// }

// type ItemsAllOf struct {
// 	Ref     *string `json:"$ref,omitempty"`
// 	Pattern *string `json:"pattern,omitempty"`
// }

// type Ref struct {
// 	Description string       `json:"description"`
// 	AllOf       []ItemsAllOf `json:"allOf"`
// }

// type Description struct {
// 	Description string   `json:"description"`
// 	Type        TypeEnum `json:"type"`
// }

// type ExecutionEnd struct {
// 	Description string `json:"description"`
// 	Ref         string `json:"$ref"`
// }

// type SpecVersion struct {
// 	Description string   `json:"description"`
// 	Type        TypeEnum `json:"type"`
// 	Const       string   `json:"const"`
// }

// type TacticRef struct {
// 	Description string `json:"description"`
// 	AllOf       []Then `json:"allOf"`
// }

// type AttackAsset struct {
// 	Description    string                `json:"description"`
// 	Type           string                `json:"type"`
// 	Properties     AttackAssetProperties `json:"properties"`
// 	Required       []string              `json:"required"`
// 	XExampleObject string                `json:"x-exampleObject"`
// }

// type AttackAssetProperties struct {
// 	Type        SpecVersion  `json:"type"`
// 	SpecVersion SpecVersion  `json:"spec_version"`
// 	Name        Description  `json:"name"`
// 	Description Description  `json:"description"`
// 	ObjectRef   ExecutionEnd `json:"object_ref"`
// }

// type AttackCondition struct {
// 	Description    string                    `json:"description"`
// 	Type           string                    `json:"type"`
// 	Properties     AttackConditionProperties `json:"properties"`
// 	Required       []string                  `json:"required"`
// 	XExampleObject string                    `json:"x-exampleObject"`
// }

// type AttackConditionProperties struct {
// 	Type           SpecVersion `json:"type"`
// 	SpecVersion    SpecVersion `json:"spec_version"`
// 	Description    Description `json:"description"`
// 	Pattern        Description `json:"pattern"`
// 	PatternType    Description `json:"pattern_type"`
// 	PatternVersion Description `json:"pattern_version"`
// 	OnTrueRefs     Refs        `json:"on_true_refs"`
// 	OnFalseRefs    Refs        `json:"on_false_refs"`
// }

// type AttackFlowClass struct {
// 	Description    string               `json:"description"`
// 	Type           string               `json:"type"`
// 	Properties     AttackFlowProperties `json:"properties"`
// 	Required       []string             `json:"required"`
// 	XExampleObject string               `json:"x-exampleObject"`
// }

// type AttackFlowProperties struct {
// 	Type        SpecVersion `json:"type"`
// 	SpecVersion SpecVersion `json:"spec_version"`
// 	Name        Description `json:"name"`
// 	Description Description `json:"description"`
// 	Scope       Scope       `json:"scope"`
// 	StartRefs   Refs        `json:"start_refs"`
// }

// type Scope struct {
// 	Description string   `json:"description"`
// 	Type        TypeEnum `json:"type"`
// 	Enum        []string `json:"enum"`
// }

// type AttackOperator struct {
// 	Description    string                   `json:"description"`
// 	Type           string                   `json:"type"`
// 	Properties     AttackOperatorProperties `json:"properties"`
// 	Required       []string                 `json:"required"`
// 	XExampleObject string                   `json:"x-exampleObject"`
// }

// type AttackOperatorProperties struct {
// 	Type        SpecVersion `json:"type"`
// 	SpecVersion SpecVersion `json:"spec_version"`
// 	Operator    Scope       `json:"operator"`
// 	EffectRefs  Refs        `json:"effect_refs"`
// }

// type TypeEnum string

// const (
// 	String TypeEnum = "string"
// )
