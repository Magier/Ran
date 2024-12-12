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
	Source    string
	Kind      string
	Target    Target
	Protocol  string
}

func (ch ImplantC2Channel) GetSource() string {
	return ch.Source
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

type KubectlExecChannel struct {
	Source string
	Cmd    string
	Target
}

func (ch KubectlExecChannel) GetSource() string {
	return ch.Source
}

func (ch KubectlExecChannel) GetTarget() string {
	return ch.Target.Name
}

func (ch KubectlExecChannel) GetRelationName() string {
	return "Kubectl exec"
}

func (ch KubectlExecChannel) GetKind() string {
	// TODO: change this to the identity to use
	return "exec"
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
