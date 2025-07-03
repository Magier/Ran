package domain

import "fmt"

type Relation interface {
	GetSourceId() string
	GetTargetId() string
	GetRelationName() string
	IsReverse() bool
}

func GetRelationId(rel Relation) string {
	return fmt.Sprintf("%s-[%s]->%s", rel.GetSourceId(), rel.GetRelationName(), rel.GetTargetId())
}

type RelationImpl struct{}

func (r RelationImpl) IsReverse() bool {
	return false
}

type Reference struct {
	RelationImpl
	Source string
	Target string
	Medium string
}

var _ Relation = (*Reference)(nil)

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
	RelationImpl
	Operator C2System
	System   C2System
}

var _ Relation = (*Operates)(nil)

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
	RelationImpl
	Container Entity
	Object    Entity
}

var _ Relation = (*Contains)(nil)

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
	RelationImpl
	Owner  Entity
	Object Ownable
}

var _ Relation = (*Owns)(nil)

func (r Owns) GetSourceId() string {
	return r.Owner.GetId()
}
func (r Owns) GetTargetId() string {
	return r.Object.GetId()
}

func (r Owns) GetRelationName() string {
	return "owns"
}

type Created struct {
	RelationImpl
	Creator Entity
	Object  Entity
}

var _ Relation = (*Created)(nil)

func (r Created) GetSourceId() string {
	return r.Creator.GetId()
}
func (r Created) GetTargetId() string {
	return r.Object.GetId()
}

func (r Created) GetRelationName() string {
	return "created"
}

type C2Channel interface {
	Relation
	GetKind() string
	GetTarget() Entity
}

type ListenesOn struct {
	RelationImpl
	Port       int
	Protocol   int
	C2ID       string
	ListenerID string
}

func (ch ListenesOn) GetSourceId() string {
	return ch.C2ID
}

func (ch ListenesOn) GetTargetId() string {
	return ch.ListenerID
}
func (ch ListenesOn) GetRelationName() string {
	return "listens-on"
}

type ImplantC2Channel struct {
	RelationImpl
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
	return fmt.Sprintf("%s-c2-%s-channel", ch.Kind, ch.Protocol)
}

func (ch ImplantC2Channel) GetKind() string {
	return ch.Kind
}

type PodExecC2Channel struct {
	RelationImpl
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
	RelationImpl
	SubjectId string
	ObjectId  string
}

var _ Relation = (*Uses)(nil)

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
	RelationImpl
	SourceId    string
	TargetId    string
	AccessLevel AccessLevel
	Identity    Identity
}

var _ Relation = (*CanAccess)(nil)

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
	return 10
}

// Utility function to provide default cost 0 for all informative relations or call the respective cost function
func GetRelationCost(relation Relation) int {
	switch r := relation.(type) {
	case CanAccess:
		return r.GetCost()
	case MountsHostPath:
		return 10
	}
	return 1000
}

type ManagesNode struct {
	RelationImpl
	Cluster Cluster
	Node    K8sNode
}

var _ Relation = (*ManagesNode)(nil)

func (r ManagesNode) GetSourceId() string {
	return r.Cluster.GetId()
}
func (r ManagesNode) GetTargetId() string {
	return r.Node.GetId()
}

func (r ManagesNode) GetRelationName() string {
	return "manages-node"
}

type Runs struct {
	RelationImpl
	Node K8sNode
	Pod  Pod
}

var _ Relation = (*Runs)(nil)

func (r Runs) IsInverse() bool {
	return true
}

func (r Runs) GetSourceId() string {
	return r.Node.GetId()
}
func (r Runs) GetTargetId() string {
	return r.Pod.GetId()
}

func (r Runs) GetRelationName() string {
	return "runs"
}

type RunsOn struct {
	RelationImpl
	Pod  Pod
	Node K8sNode
}

var _ Relation = (*RunsOn)(nil)

func (r RunsOn) IsInverse() bool {
	return true
}

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
	RelationImpl
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

type BindsRole struct {
	RelationImpl
	// TODO: generalize this to User and Groups
	Subject ServiceAccount
	Role    Role
}

var _ Relation = (*BindsRole)(nil)

func (r BindsRole) IsInverse() bool {
	return true
}

func (r BindsRole) GetSourceId() string {
	return r.Subject.GetId()
}
func (r BindsRole) GetTargetId() string {
	return r.Role.GetId()
}

func (r BindsRole) GetRelationName() string {
	return "binds"
}

type MountsHostPath struct {
	RelationImpl
	Pod       Pod
	Node      K8sNode
	HostPath  string
	MountPath string
}

var _ Relation = (*MountsHostPath)(nil)

func (r MountsHostPath) IsInverse() bool {
	return true
}

func (r MountsHostPath) GetSourceId() string {
	return r.Pod.GetId()
}
func (r MountsHostPath) GetTargetId() string {
	return r.Node.GetId()
}

func (r MountsHostPath) GetRelationName() string {
	return fmt.Sprintf("mounts %s", r.HostPath)
}
