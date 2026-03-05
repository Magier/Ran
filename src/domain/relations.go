package domain

import (
	"fmt"
	"log/slog"
	"net/url"
	"strings"

	"github.com/google/shlex"
)

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
	Medium string `json:"medium,omitzero"`
}

var _ Relation = (*Reference)(nil)

func (r Reference) GetSourceId() string     { return r.Source }
func (r Reference) GetTargetId() string     { return r.Target }
func (r Reference) GetRelationName() string { return "references" }

func (r Reference) WithSource(e Entity) Relation {
	r.Source = e.GetId()
	return r
}
func (r Reference) WithTarget(e Entity) Relation {
	r.Target = e.GetId()
	return r
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

func (r Operates) GetSourceId() string     { return r.Operator.GetId() }
func (r Operates) GetTargetId() string     { return r.System.GetId() }
func (r Operates) GetRelationName() string { return "operates" }

func (r Operates) WithSource(e Entity) Relation {
	if c2, ok := e.(C2System); ok {
		r.Operator = c2
	} else {
		slog.Warn("WithSource called with non-C2System entity", "entity", e.GetId())
	}
	return r
}
func (r Operates) WithTarget(e Entity) Relation {
	if c2, ok := e.(C2System); ok {
		r.System = c2
	} else {
		slog.Warn("WithTarget called with non-C2System entity", "entity", e.GetId())
	}
	return r
}

type Contains struct {
	RelationImpl
	Container Entity
	Object    Entity
}

var _ Relation = (*Contains)(nil)

func (r Contains) GetSourceId() string     { return r.Container.GetId() }
func (r Contains) GetTargetId() string     { return r.Object.GetId() }
func (r Contains) GetRelationName() string { return "contains" }

// Optional: expose ends as Entities (handy for helpers)
func (r Contains) GetSource() Entity { return r.Container }
func (r Contains) GetTarget() Entity { return r.Object }

// --- copy-style modifiers (return a new value) ---
func (r Contains) WithSource(e Entity) Relation { r.Container = e; return r }
func (r Contains) WithTarget(e Entity) Relation { r.Object = e; return r }
func (r Contains) WithEnds(src, tgt Entity) Relation {
	r.Container, r.Object = src, tgt
	return r
}

type Owns struct {
	RelationImpl
	Owner  Entity
	Object Ownable
}

var _ Relation = (*Owns)(nil)

func (r Owns) GetSourceId() string     { return r.Owner.GetId() }
func (r Owns) GetTargetId() string     { return r.Object.GetId() }
func (r Owns) GetRelationName() string { return "owns" }

func (r Owns) WithSource(e Entity) Relation { r.Owner = e; return r }
func (r Owns) WithTarget(e Entity) Relation {
	if ownable, ok := e.(Ownable); ok {
		r.Object = ownable
	} else {
		slog.Warn("WithTarget called with non-Ownable entity", "entity", e.GetId())
	}
	return r
}

type Created struct {
	RelationImpl
	Creator Entity
	Object  Entity
}

var _ Relation = (*Created)(nil)

func (r Created) GetSourceId() string          { return r.Creator.GetId() }
func (r Created) GetTargetId() string          { return r.Object.GetId() }
func (r Created) GetRelationName() string      { return "created" }
func (r Created) WithSource(e Entity) Relation { r.Creator = e; return r }
func (r Created) WithTarget(e Entity) Relation { r.Object = e; return r }

// TODO: generalize C2 channel with multiple segments
type C2Channel interface {
	Relation
	GetKind() string
	GetTarget() Entity
	GetFinalTarget() Entity
	SetNextChannel(ch C2Channel)
	GetNextChannel() C2Channel
	GetCommandEnvelope(cmd string) string
}

// type ListenesOn struct {
// 	RelationImpl
// 	Port       int
// 	Protocol   int
// 	C2ID       string
// 	ListenerID string
// }

// func (ch ListenesOn) GetSourceId() string     { return ch.C2ID }
// func (ch ListenesOn) GetTargetId() string     { return ch.ListenerID }
// func (ch ListenesOn) GetRelationName() string { return "listens-on" }

// func (ch ListenesOn) WithSource(e Entity) Relation {
// 	if c2, ok := e.(C2System); ok {
// 		ch.C2ID = c2.GetId()
// 	} else {
// 		slog.Warn("WithSource called with non-C2System entity", "entity", e.GetId())
// 	}
// 	return ch
// }
// func (ch ListenesOn) WithTarget(e Entity) Relation {
// 	ch.ListenerID = e.GetId()
// 	return ch
// }

type ImplantC2Channel struct {
	RelationImpl
	SessionId   string
	SourceId    string
	Kind        string
	Target      Session
	Protocol    string
	NextChannel C2Channel
}

var _ C2Channel = (*ImplantC2Channel)(nil)

func (ch *ImplantC2Channel) GetSourceId() string { return ch.SourceId }
func (ch *ImplantC2Channel) GetTargetId() string { return ch.Target.GetId() }
func (ch *ImplantC2Channel) GetRelationName() string {
	return fmt.Sprintf("%s-c2-%s-channel", ch.Kind, ch.Protocol)
}

func (ch *ImplantC2Channel) WithSource(e Entity) Relation {
	ch.SourceId = e.GetId()
	return ch
}
func (ch *ImplantC2Channel) WithTarget(e Entity) Relation {
	if session, ok := e.(Session); ok {
		ch.Target = session
	} else {
		slog.Warn("WithTarget called with non-Session entity", "entity", e.GetId())
	}
	return ch
}

func (ch *ImplantC2Channel) GetTarget() Entity { return ch.Target }
func (ch *ImplantC2Channel) GetFinalTarget() Entity {
	slog.Warn("GetFinalTarget is not yet supported on ImplantC2Channel, using the next target instead!")
	return ch.Target
}
func (ch *ImplantC2Channel) GetKind() string                      { return ch.Kind }
func (ch *ImplantC2Channel) GetCommandEnvelope(cmd string) string { return cmd }
func (ch *ImplantC2Channel) GetNextChannel() C2Channel            { return ch.NextChannel }
func (ch *ImplantC2Channel) SetNextChannel(next C2Channel)        { ch.NextChannel = next }

type PodExecC2Channel struct {
	RelationImpl
	SourceId string
	// Cmd    string
	Target        Entity
	Identity      Identity
	IsInteractive bool
	NextChannel   C2Channel // for chaining multiple
}

var _ C2Channel = (*PodExecC2Channel)(nil)

func (ch *PodExecC2Channel) GetSourceId() string     { return ch.SourceId }
func (ch *PodExecC2Channel) GetTargetId() string     { return ch.Target.GetId() }
func (ch *PodExecC2Channel) GetRelationName() string { return "pod exec" }

func (ch *PodExecC2Channel) WithSource(e Entity) Relation {
	ch.SourceId = e.GetId()
	return ch
}
func (ch *PodExecC2Channel) WithTarget(e Entity) Relation {
	ch.Target = e
	return ch
}

func (ch *PodExecC2Channel) GetCommandEnvelope(cmd string) string {
	return fmt.Sprintf("kubectl exec %s -- %s", ch.Target.GetName(), cmd)
}

func (ch *PodExecC2Channel) GetTarget() Entity { return ch.Target }
func (ch *PodExecC2Channel) GetKind() string   { return "pod/exec" }

func (ch *PodExecC2Channel) GetFinalTarget() Entity {
	target := ch.Target
	for next := ch.GetNextChannel(); next != nil; next = next.GetNextChannel() {
		target = next.GetTarget()
	}
	return target
}
func (ch *PodExecC2Channel) GetNextChannel() C2Channel     { return ch.NextChannel }
func (ch *PodExecC2Channel) SetNextChannel(next C2Channel) { ch.NextChannel = next }

type Uses struct {
	RelationImpl
	SubjectId string
	ObjectId  string
}

var _ Relation = (*Uses)(nil)

func (u Uses) GetSourceId() string     { return u.SubjectId }
func (u Uses) GetTargetId() string     { return u.ObjectId }
func (u Uses) GetRelationName() string { return "uses" }

func (u Uses) WithSource(e Entity) Relation {
	u.SubjectId = e.GetId()
	return u
}
func (u Uses) WithTarget(e Entity) Relation {
	u.ObjectId = e.GetId()
	return u
}

type CanAccess struct {
	RelationImpl
	SourceId    string
	TargetId    string
	AccessLevel AccessLevel
	Identity    Identity
	PodsExec    bool
}

var _ Relation = (*CanAccess)(nil)

func (u CanAccess) GetSourceId() string     { return u.SourceId }
func (u CanAccess) GetTargetId() string     { return u.TargetId }
func (u CanAccess) GetRelationName() string { return "can-access" }

func (u CanAccess) WithSource(e Entity) Relation {
	u.SourceId = e.GetId()
	return u
}
func (u CanAccess) WithTarget(e Entity) Relation {
	u.TargetId = e.GetId()
	return u
}

func (u CanAccess) GetCost() int {
	return 10
}

// GetRelationCost returns a cost for path-finding. Actionable relations
// (ones that represent real attack primitives) get low costs so the
// shortest-path algorithm prefers them over purely informational edges.
func GetRelationCost(relation Relation) int {
	switch r := relation.(type) {
	case CanAccess:
		return r.GetCost()
	case *PodExecC2Channel:
		return 5
	case *ImplantC2Channel:
		return 5
	case *KubeletExecSource:
		return 10
	case *KubeletExecSink:
		return 10
	case MountsHostPaths:
		return 10
	case CanReach:
		return 20
	case ExposesSecret:
		return 15
	case BindsRole:
		return 20
	// Informational / structural relations — high cost so they are
	// only used when no actionable path exists.
	case RunsOn, Runs, Contains, Owns, Created, ManagesNode, Uses,
		HasC2Session, Operates, Reference:
		return 1000
	}
	return 1000
}

type ManagesNode struct {
	RelationImpl
	Cluster Cluster
	Node    K8sNode
}

var _ Relation = (*ManagesNode)(nil)

func (r ManagesNode) GetSourceId() string     { return r.Cluster.GetId() }
func (r ManagesNode) GetTargetId() string     { return r.Node.GetId() }
func (r ManagesNode) GetRelationName() string { return "manages-node" }

func (r ManagesNode) WithSource(e Entity) Relation {
	if cluster, ok := e.(Cluster); ok {
		r.Cluster = cluster
	} else {
		slog.Warn("WithSource called with non-Cluster entity", "entity", e.GetId())
	}
	return r
}
func (r ManagesNode) WithTarget(e Entity) Relation {
	if node, ok := e.(K8sNode); ok {
		r.Node = node
	} else {
		slog.Warn("WithTarget called with non-K8sNode entity", "entity", e.GetId())
	}
	return r
}

type Runs struct {
	RelationImpl
	Node K8sNode
	Pod  Pod
}

var _ Relation = (*Runs)(nil)

func (r Runs) IsInverse() bool         { return true }
func (r Runs) GetSourceId() string     { return r.Node.GetId() }
func (r Runs) GetTargetId() string     { return r.Pod.GetId() }
func (r Runs) GetRelationName() string { return "runs" }

func (r Runs) WithSource(e Entity) Relation {
	if node, ok := e.(K8sNode); ok {
		r.Node = node
	} else {
		slog.Warn("WithSource called with non-K8sNode entity", "entity", e.GetId())
	}
	return r
}
func (r Runs) WithTarget(e Entity) Relation {
	if pod, ok := e.(Pod); ok {
		r.Pod = pod
	} else {
		slog.Warn("WithTarget called with non-Pod entity", "entity", e.GetId())
	}
	return r
}

type RunsOn struct {
	RelationImpl
	Pod  Pod
	Node K8sNode
}

var _ Relation = (*RunsOn)(nil)

func (r RunsOn) IsInverse() bool         { return true }
func (r RunsOn) GetSourceId() string     { return r.Pod.GetId() }
func (r RunsOn) GetTargetId() string     { return r.Node.GetId() }
func (r RunsOn) GetRelationName() string { return "runs-on" }

func (r RunsOn) WithSource(e Entity) Relation {
	if pod, ok := e.(Pod); ok {
		r.Pod = pod
	} else {
		slog.Warn("WithSource called with non-Pod entity", "entity", e.GetId())
	}
	return r
}
func (r RunsOn) WithTarget(e Entity) Relation {
	if node, ok := e.(K8sNode); ok {
		r.Node = node
	} else {
		slog.Warn("WithTarget called with non-K8sNode entity", "entity", e.GetId())
	}
	return r
}

type HasC2Session struct {
	RelationImpl
	System  Entity
	Session Session
}

var _ Relation = (*HasC2Session)(nil)

func (r HasC2Session) GetSourceId() string     { return r.System.GetId() }
func (r HasC2Session) GetTargetId() string     { return r.Session.Id }
func (r HasC2Session) GetRelationName() string { return "has-session" }
func (r HasC2Session) IsReverse() bool         { return true }

func (r HasC2Session) WithSource(e Entity) Relation {
	r.System = e
	return r
}
func (r HasC2Session) WithTarget(e Entity) Relation {
	if session, ok := e.(Session); ok {
		r.Session = session
	} else {
		slog.Warn("WithTarget called with non-Session entity", "entity", e.GetId())
	}
	return r
}

type BindsRole struct {
	RelationImpl
	Subject     Identity
	Role        Role
	RoleBinding RoleBinding
}

var _ Relation = (*BindsRole)(nil)

func (r BindsRole) IsInverse() bool         { return true }
func (r BindsRole) GetSourceId() string     { return r.Subject.GetId() }
func (r BindsRole) GetTargetId() string     { return r.Role.GetId() }
func (r BindsRole) GetRelationName() string { return "binds" }

func (r BindsRole) WithSource(e Entity) Relation {
	if identity, ok := e.(Identity); ok {
		r.Subject = identity
	} else {
		slog.Warn("WithSource called with non-Identity entity", "entity", e.GetId())
	}
	return r
}
func (r BindsRole) WithTarget(e Entity) Relation {
	if role, ok := e.(Role); ok {
		r.Role = role
	} else {
		slog.Warn("WithTarget called with non-Role entity", "entity", e.GetId())
	}
	return r
}

type ExposesSecret struct {
	RelationImpl
	Object Entity
	Secret Secret
}

var _ Relation = (*ExposesSecret)(nil)

func (r ExposesSecret) GetSourceId() string     { return r.Object.GetId() }
func (r ExposesSecret) GetTargetId() string     { return r.Secret.Name }
func (r ExposesSecret) GetRelationName() string { return "exposes-secret" }

func (r ExposesSecret) WithSource(e Entity) Relation {
	r.Object = e
	return r
}
func (r ExposesSecret) WithTarget(e Entity) Relation {
	if secret, ok := e.(Secret); ok {
		r.Secret = secret
	} else {
		slog.Warn("WithTarget called with non-Secret entity", "entity", e.GetId())
	}
	return r
}

type MountsHostPaths struct {
	RelationImpl
	Pod       Pod
	Node      K8sNode
	HostPaths map[string]string
}

var _ Relation = (*MountsHostPaths)(nil)

func NewMountsHostPathsRelation(pod Pod, node K8sNode) MountsHostPaths {
	mounts := make(map[string]string)

	return MountsHostPaths{
		Pod:       pod,
		Node:      node,
		HostPaths: mounts,
	}
}

func (r *MountsHostPaths) AddMount(mountPath, hostPath string) {
	r.HostPaths[mountPath] = hostPath
}

func (r MountsHostPaths) IsInverse() bool     { return true }
func (r MountsHostPaths) GetSourceId() string { return r.Pod.GetId() }
func (r MountsHostPaths) GetTargetId() string { return r.Node.GetId() }
func (r MountsHostPaths) GetRelationName() string {
	if len(r.HostPaths) == 1 {
		for _, hostPath := range r.HostPaths {
			return fmt.Sprintf("mounts %s", hostPath)
		}
	}
	return fmt.Sprintf("%d hostPaths", len(r.HostPaths))
}

func (r MountsHostPaths) WithSource(e Entity) Relation {
	if pod, ok := e.(Pod); ok {
		r.Pod = pod
	} else {
		slog.Warn("WithSource called with non-Pod entity", "entity", e.GetId())
	}
	return r
}
func (r MountsHostPaths) WithTarget(e Entity) Relation {
	if node, ok := e.(K8sNode); ok {
		r.Node = node
	} else {
		slog.Warn("WithTarget called with non-K8sNode entity", "entity", e.GetId())
	}
	return r
}

type CanReach struct {
	RelationImpl
	SourceId string
	TargetId string
	Address  string
}

var _ Relation = (*CanReach)(nil)

func (r CanReach) GetSourceId() string     { return r.SourceId }
func (r CanReach) GetTargetId() string     { return r.TargetId }
func (r CanReach) GetRelationName() string { return "can-reach" }

func (r CanReach) WithSource(e Entity) Relation {
	r.SourceId = e.GetId()
	return r
}
func (r CanReach) WithTarget(e Entity) Relation {
	r.TargetId = e.GetId()
	return r
}

// KubeletExecSource represents a pod's ability to execute commands on its hosting node via the kubelet API.
// Direction: Pod → Node (source of the C2 channel, holds the identity/token).
type KubeletExecSource struct {
	RelationImpl
	Pod         Pod
	Node        K8sNode
	Identity    Identity
	Procedure   Procedure
	NextChannel C2Channel
}

var _ Relation = (*KubeletExecSource)(nil)
var _ C2Channel = (*KubeletExecSource)(nil)

func (r *KubeletExecSource) GetSourceId() string     { return r.Pod.GetId() }
func (r *KubeletExecSource) GetTargetId() string     { return r.Node.GetId() }
func (r *KubeletExecSource) GetRelationName() string { return "kubelet-exec" }

func (r *KubeletExecSource) WithSource(e Entity) Relation {
	if pod, ok := e.(Pod); ok {
		r.Pod = pod
	} else {
		slog.Warn("WithSource called with non-Pod entity", "entity", e.GetId())
	}
	return r
}
func (r *KubeletExecSource) WithTarget(e Entity) Relation {
	if node, ok := e.(K8sNode); ok {
		r.Node = node
	} else {
		slog.Warn("WithTarget called with non-K8sNode entity", "entity", e.GetId())
	}
	return r
}

func (r *KubeletExecSource) GetKind() string   { return "kubelet/exec" }
func (r *KubeletExecSource) GetTarget() Entity { return r.Node }
func (r *KubeletExecSource) GetFinalTarget() Entity {
	target := Entity(r.Node)
	for next := r.GetNextChannel(); next != nil; next = next.GetNextChannel() {
		target = next.GetTarget()
	}
	return target
}
func (r *KubeletExecSource) GetNextChannel() C2Channel     { return r.NextChannel }
func (r *KubeletExecSource) SetNextChannel(next C2Channel) { r.NextChannel = next }
func (r *KubeletExecSource) GetCommandEnvelope(cmd string) string {
	// Substitute the ${TOKEN} placeholder in the command produced by the sink
	return strings.ReplaceAll(cmd, "${TOKEN}", r.Identity.GetToken())
}

// KubeletExecSink represents the ability to exec into a pod on a node via the kubelet API.
// Direction: Node → Pod (sink of the C2 channel, holds the target pod/node info).
type KubeletExecSink struct {
	RelationImpl
	Node        K8sNode
	Pod         Pod
	Procedure   Procedure
	NextChannel C2Channel
}

var _ Relation = (*KubeletExecSink)(nil)
var _ C2Channel = (*KubeletExecSink)(nil)

func (r *KubeletExecSink) GetSourceId() string     { return r.Node.GetId() }
func (r *KubeletExecSink) GetTargetId() string     { return r.Pod.GetId() }
func (r *KubeletExecSink) GetRelationName() string { return "kubelet-pod-exec" }

func (r *KubeletExecSink) WithSource(e Entity) Relation {
	if node, ok := e.(K8sNode); ok {
		r.Node = node
	} else {
		slog.Warn("WithSource called with non-K8sNode entity", "entity", e.GetId())
	}
	return r
}
func (r *KubeletExecSink) WithTarget(e Entity) Relation {
	if pod, ok := e.(Pod); ok {
		r.Pod = pod
	} else {
		slog.Warn("WithTarget called with non-Pod entity", "entity", e.GetId())
	}
	return r
}

func (r *KubeletExecSink) GetKind() string   { return "kubelet/pod-exec" }
func (r *KubeletExecSink) GetTarget() Entity { return r.Pod }
func (r *KubeletExecSink) GetFinalTarget() Entity {
	target := Entity(r.Pod)
	for next := r.GetNextChannel(); next != nil; next = next.GetNextChannel() {
		target = next.GetTarget()
	}
	return target
}
func (r *KubeletExecSink) GetNextChannel() C2Channel     { return r.NextChannel }
func (r *KubeletExecSink) SetNextChannel(next C2Channel) { r.NextChannel = next }
func (r *KubeletExecSink) GetCommandEnvelope(cmd string) string {
	container := r.Pod.Containers[0].Name // TODO find out how to select the right container if there are multiple

	// Tokenize with shell semantics (respects quotes, escapes) and join as
	// separate &command= query params for the kubelet exec API.
	tokens, _ := shlex.Split(cmd)
	for i, t := range tokens {
		tokens[i] = url.QueryEscape(t)
	}
	cmdParams := strings.Join(tokens, "&command=")
	var node string
	if len(r.Node.IPs) > 0 {
		node = r.Node.IPs[0].String()
	} else {
		node = r.Node.GetName()
	}

	// Substitute parameters into the Procedure's command template
	// TODO: the grounding should be done in the campaign alongside the other arguments
	// these are practically nested arguments -> needs support for TTP nesting
	result := r.Procedure.Command
	result = strings.ReplaceAll(result, "${NODE}", node)
	result = strings.ReplaceAll(result, "${NAMESPACE}", r.Pod.GetNamespace())
	result = strings.ReplaceAll(result, "${POD_NAME}", r.Pod.GetName())
	result = strings.ReplaceAll(result, "${CONTAINER}", container)
	result = strings.ReplaceAll(result, "${COMMAND}", cmdParams)
	return result
}
