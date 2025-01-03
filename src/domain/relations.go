package domain

import "fmt"

type Relation interface {
	GetSource() string
	GetTarget() string
	GetRelationName() string
}

func GetRelationId(rel Relation) string {
	return fmt.Sprintf("%s-[%s]->%s", rel.GetSource(), rel.GetRelationName(), rel.GetTarget())
}

type Reference struct {
	Source string
	Target string
	Medium string
}

func (r Reference) GetSource() string {
	return r.Source
}
func (r Reference) GetTarget() string {
	return r.Target
}

func (r Reference) GetRelationName() string {
	return "references"
}

func (r Reference) String() string {
	return "extracted from " + r.Medium
}

type Contains struct {
	Container Entity
	Object    Entity
}

func (r Contains) GetSource() string {
	return r.Container.GetName()
}

func (r Contains) GetTarget() string {
	return r.Object.GetName()
}

func (r Contains) GetRelationName() string {
	return "contains"
}

type Owns struct {
	Owner  Entity
	Object Ownable
}

func (r Owns) GetSource() string {
	return r.Owner.GetId()
}
func (r Owns) GetTarget() string {
	return r.Object.GetId()
}

func (r Owns) GetRelationName() string {
	return "owns"
}

type C2Channel interface {
	Relation
	GetKind() string
}

type ImplantC2Channel struct {
	SessionId string
	SourceId  string
	Kind      string
	Target    Target
	Protocol  string
}

func (ch ImplantC2Channel) GetSource() string {
	return ch.SourceId
}

func (ch ImplantC2Channel) GetTarget() string {
	return ch.Target.Id
}

func (ch ImplantC2Channel) GetRelationName() string {
	return fmt.Sprintf("Implant %s %s Channel", ch.Kind, ch.Protocol)
}

func (ch ImplantC2Channel) GetKind() string {
	return ch.Kind
}

type PodExecC2Channel struct {
	SourceId string
	// Cmd    string
	TargetId string
	Identity Identity
}

func (ch PodExecC2Channel) GetSource() string {
	return ch.SourceId
}

func (ch PodExecC2Channel) GetTarget() string {
	return ch.TargetId
}

func (ch PodExecC2Channel) GetRelationName() string {
	return "pod exec"
}

func (ch PodExecC2Channel) GetKind() string {
	// TODO: change this to the identity to use
	return "pod/exec"
}

type Uses struct {
	SubjectId string
	ObjectId  string
}

func (u Uses) GetSource() string {
	return u.SubjectId
}

func (u Uses) GetTarget() string {
	return u.ObjectId
}

func (u Uses) GetRelationName() string {
	return "uses"
}

type CanAccess struct {
	SourceId    string
	TargetId    string
	AccessLevel AccessLevel
	Identity    Identity
}

func (u CanAccess) GetSource() string {
	return u.SourceId
}

func (u CanAccess) GetTarget() string {
	return u.TargetId
}

func (u CanAccess) GetRelationName() string {
	return "can-access"
}
func (u CanAccess) GetCost() int {
	return 100
}

// Utility function to provide default cost 0 for all informative relations or call the respective cost function
func GetRelationCost(relation Relation) int {
	switch r := relation.(type) {
	case CanAccess:
		return r.GetCost()
	}
	return 0
}
