package domain

import "fmt"

type Relation interface {
	GetSourceId() string
	GetTargetId() string
	GetRelationName() string
}

func GetRelationId(rel Relation) string {
	return fmt.Sprintf("%s-[%s]->%s", rel.GetSourceId(), rel.GetRelationName(), rel.GetTargetId())
}

type Reference struct {
	Source string
	Target string
	Medium string
}

func (r Reference) GetSourceId() string {
	return r.Source
}
func (r Reference) GetTargetId() string {
	return r.Target
}

func (r Reference) GetRelationName() string {
	return "references"
}

func (r Reference) String() string {
	return "extracted from " + r.Medium
}

type Operates struct {
	Operator C2System
	System   C2System
}

func (r Operates) GetSourceId() string {
	return r.Operator.GetId()
}
func (r Operates) GetTargetId() string {
	return r.System.GetId()
}

func (r Operates) GetRelationName() string {
	return "operates"
}

type Contains struct {
	Container Entity
	Object    Entity
}

func (r Contains) GetSourceId() string {
	return r.Container.GetId()
}

func (r Contains) GetTargetId() string {
	return r.Object.GetId()
}

func (r Contains) GetRelationName() string {
	return "contains"
}

type Owns struct {
	Owner  Entity
	Object Ownable
}

func (r Owns) GetSourceId() string {
	return r.Owner.GetId()
}
func (r Owns) GetTargetId() string {
	return r.Object.GetId()
}

func (r Owns) GetRelationName() string {
	return "owns"
}

type C2Channel interface {
	Relation
	GetKind() string
	GetTarget() Entity
}

type ImplantC2Channel struct {
	SessionId string
	SourceId  string
	Kind      string
	Target    Entity
	Protocol  string
}

var _ C2Channel = (*ImplantC2Channel)(nil)

func (ch ImplantC2Channel) GetSourceId() string {
	return ch.SourceId
}

func (ch ImplantC2Channel) GetTargetId() string {
	return ch.Target.GetId()
}

func (ch ImplantC2Channel) GetTarget() Entity {
	return ch.Target
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
	Target   Entity
	Identity Identity
}

var _ C2Channel = (*PodExecC2Channel)(nil)

func (ch PodExecC2Channel) GetSourceId() string {
	return ch.SourceId
}

func (ch PodExecC2Channel) GetTargetId() string {
	return ch.Target.GetId()
}
func (ch PodExecC2Channel) GetTarget() Entity {
	return ch.Target
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

func (u Uses) GetSourceId() string {
	return u.SubjectId
}

func (u Uses) GetTargetId() string {
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

func (u CanAccess) GetSourceId() string {
	return u.SourceId
}

func (u CanAccess) GetTargetId() string {
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

type RunsOn struct {
	Pod  Pod
	Node K8sNode
}

var _ Relation = (*RunsOn)(nil)

func (r RunsOn) GetSourceId() string {
	return r.Pod.GetId()
}
func (r RunsOn) GetTargetId() string {
	return r.Node.GetId()
}

func (r RunsOn) GetRelationName() string {
	return "runs-on"
}

type HasC2Session struct {
	System  Entity
	Session Session
}

var _ Relation = (*HasC2Session)(nil)

func (r HasC2Session) GetSourceId() string {
	return r.System.GetId()
}
func (r HasC2Session) GetTargetId() string {
	return r.Session.Id
}

func (r HasC2Session) GetRelationName() string {
	return "has-session"
}
