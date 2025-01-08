package domain

import (
	"fmt"
	"net"

	v1 "k8s.io/api/core/v1"
)

type Protocol string

const (
	ANY   Protocol = "ANY"
	TCP   Protocol = "TCP"
	UDP   Protocol = "UDP"
	HTTP  Protocol = "HTTP"
	HTTPS Protocol = "HTTPS"
	DNS   Protocol = "DNS"
	mTLS  Protocol = "mTLS"
)

type Condition interface {
	Satisfies(Condition) bool
	IsSet() bool
}

type Requirements struct {
	Kind           IsOfKind
	AccessLevel    AccessLevel
	RbacPermission Permission
	Infra          []string
	State          State        // check for existing entities
	Exists         EntityExists // relates to the state
}

func (r Requirements) Satisfied(target Entity, accessLevel AccessLevel, state State) bool {
	if r.Kind != "" && r.Kind != IsOfKind(target.GetKind()) {
		return false
	}
	if !accessLevel.Satisfies(r.AccessLevel) {
		return false
	}

	if r.RbacPermission != "" {
		return false
	}

	if len(r.State) > 0 {
		if !state.Satisfies(r.State) {
			return false
		}
	}
	return true
}

type IsOfKind string

func (k IsOfKind) Satisfies(r Condition) bool {
	if kind, ok := r.(IsOfKind); ok {
		return k == kind
	}
	return false
}
func (k IsOfKind) IsSet() bool {
	return k != ""
}

var _ Condition = (*IsOfKind)(nil)

type State map[string]int

func (s State) Satisfies(r Condition) bool {
	if entityKind, ok := r.(EntityExists); ok {
		numExists, existsOk := s[string(entityKind)]
		return existsOk && numExists > 0
	}
	return false
}
func (s State) IsSet() bool {
	return false
}
func (s State) Update(key string, numChange int) State {
	prevNum, exists := s[key]
	if !exists {
		prevNum = 0
	}

	s[key] = prevNum + numChange
	return s
}

var _ Condition = (*State)(nil)

type AccessLevel struct {
	user  int // 0 = none, 1 = user, 2 = root
	level int // 0 = none, 1 = read, 2 = exec
}

func (lvl AccessLevel) Satisfies(requirement Condition) bool {
	if r, ok := requirement.(AccessLevel); ok {
		return r.user <= lvl.user && r.level <= lvl.level
	}
	return false
}
func (lvl AccessLevel) IsSet() bool {
	return lvl != NoAccess
}

var _ Condition = (*AccessLevel)(nil)

func (lvl AccessLevel) String() string {
	switch lvl {
	case UserRead:
		return "user-read"
	case UserExec:
		return "user-exec"
	case RootRead:
		return "root-read"
	case RootExec:
		return "root-exec"
	}
	return ""
}

var (
	NoAccess = AccessLevel{user: 0, level: 0}
	UserRead = AccessLevel{user: 1, level: 1}
	UserExec = AccessLevel{user: 1, level: 2}
	RootRead = AccessLevel{user: 2, level: 1}
	RootExec = AccessLevel{user: 2, level: 2}
)

type Permission string

func (p Permission) Satisfies(r Condition) bool {
	return false
}

func (p Permission) IsSet() bool {
	return false
}

type EntityExists string

func (e EntityExists) Satisfies(requirement Condition) bool {
	return false
}
func (e EntityExists) IsSet() bool {
	return e != ""
}

var _ Condition = (*AccessLevel)(nil)

type IdentityType string

const (
	AdminUser        IdentityType = "AdminUser"
	User             IdentityType = "User"
	ServiceAccountId IdentityType = "ServiceAccount"
)

type Listener struct {
	ID         string
	Port       uint
	Protocol   Protocol
	Redirector string
	IP         net.IP
}

type Workload interface {
	Entity
	GetPods() []Pod
}

type AbstractWorkload struct {
	K8sEntity
	// NamespacedResource
	ResourceOwner
}

func (wl AbstractWorkload) GetId() string {
	return fmt.Sprintf("ns/%s/wl/%s", wl.GetNamespace(), wl.GetName())
}

func (wl AbstractWorkload) GetKind() string {
	return "AbstractWorkload"
}

//	func (wl AbstractWorkload) GetPods() []Pod {
//		return wl.Pods
//	}
func (wl AbstractWorkload) IsAbstract() bool {
	return true
}

type ResourceOwner struct {
	Pods []Pod
}

func (w ResourceOwner) GetPods() []Pod {
	return w.Pods
}

type Entity interface {
	GetId() string
	GetName() string
	GetKind() string
}

type Ownable interface {
	GetId() string
	GetOwner() (OwnerRef, bool)
	SetOwner(name, kind string) OwnerRef
}

type System struct {
	Name        string
	OS          string
	IP          net.IP
	AccessLevel AccessLevel
}

func (s System) GetId() string {
	return "system/" + s.Name
}
func (s System) GetName() string {
	return s.Name
}

func (s System) GetKind() string {
	return "System"
}

type C2System struct {
	Kind string
	Name string
	IP   net.IP
}

func (s C2System) GetId() string {
	return "c2/" + s.Kind
}
func (s C2System) GetName() string {
	return s.Name
}

func (s C2System) GetKind() string {
	return "C2"
}

type Namespaced interface {
	GetNamespace() string
}

type EntityPlaceholder interface {
	IsAbstract() bool
}

func GenerateId(name, kind, ns string) string {
	kindShortName := GetResourceShortName(kind)
	id := "/" + kindShortName + "/" + name
	// if it doesn't start with "ns/" then ID has pattern "/kind", which equate to a clusterwide resource
	if ns != "" {
		id = "ns/" + ns + id
	}
	return id
}

type OwnerRef struct {
	Name string
	Kind string
	Uid  string
}

type K8sEntity struct {
	Id          string
	Name        string
	Kind        string
	Namespace   string
	Labels      map[string]string
	Annotations map[string]string
	CreatedAt   string
	Owner       OwnerRef
	IP          net.IP
	AccessLevel AccessLevel
}

func NewK8sEntity(name, kind, namespace string) K8sEntity {
	return K8sEntity{
		Name:        name,
		Kind:        kind,
		Namespace:   namespace,
		AccessLevel: NoAccess,
		Labels:      make(map[string]string),
		Annotations: make(map[string]string),
		// TODO set createdAt here
	}
}

func (e K8sEntity) GetId() string {
	return GenerateId(e.Name, e.Kind, e.Namespace)
}

func (e K8sEntity) GetName() string {
	return e.Name
}

func (e K8sEntity) GetKind() string {
	return e.Kind
}

func (e K8sEntity) GetLabel(label string) (string, bool) {
	if e.Labels != nil {
		v, ok := e.Labels[label]
		return v, ok
	}
	return "", false
}
func (e K8sEntity) GetOwner() (OwnerRef, bool) {
	if e.Owner.Name != "" {
		return e.Owner, true
	}
	return OwnerRef{}, false
}
func (e K8sEntity) SetOwner(name, kind string) OwnerRef {
	return OwnerRef{
		Name: name,
		Kind: kind,
	}
}

func (e K8sEntity) GetNamespace() string {
	return e.Namespace
}

func (e K8sEntity) IsNamespaced() bool {
	return e.Namespace != ""
}

// type NamespacedResource struct {
// 	Namespace string
// }

// func (n NamespacedResource) GetNamespace() string {
// 	return n.Namespace
// }

type ApiServer struct {
	Pod
	CAData     []byte
	ExternalIP net.IPAddr
}

type Namespace struct {
	Name string
}

var _ Entity = (*Namespace)(nil)

func (ns Namespace) GetId() string {
	return "ns/" + ns.Name
}

func (ns Namespace) GetName() string {
	return ns.Name
}

func (ns Namespace) GetKind() string {
	return "Namespace"
}

type RbacPermission struct {
	Verbs         []string
	Scope         string // "" is invalid, "*" =cluster-wide, any string = namespaces
	ResourceTypes []string
	ResourceNames []string
}

type Identity struct {
	Name        string
	Kind        IdentityType
	CertData    []byte
	KeyData     []byte
	Permissions []RbacPermission
}

func (id Identity) Can(permission string) bool {
	for _, perm := range id.Permissions {
		for _, v := range perm.Verbs {
			// TODO: properly filter for scope, resource name/type + wildcards
			if v == permission || v == "*" {
				return true
			}
		}
	}
	return false
}

type Pod struct {
	K8sEntity
	// NamespacedResource
	Spec    v1.PodSpec
	IP      net.IPAddr
	EnvVars map[string]string
}

func NewPod(name, ns string) Pod {
	entity := NewK8sEntity(name, "Pod", ns)
	return Pod{K8sEntity: entity}
}

type Deployment struct {
	K8sEntity
	// NamespacedResource
	ResourceOwner
}

func NewDeployment(name, ns string) Deployment {
	return Deployment{
		K8sEntity: K8sEntity{
			Name:      name,
			Namespace: ns,
			Kind:      "Deployment",
		},
	}
}

type Service struct {
	K8sEntity
	// NamespacedResource
	Targets []string
	Host    string
	FQDN    string
	Ports   map[string]int
}

type ReplicaSet struct {
	K8sEntity
	ResourceOwner
	// NamespacedResource
}

type StatefulSet struct {
	K8sEntity
	ResourceOwner
	// NamespacedResource
}

type DaemonSet struct {
	K8sEntity
	ResourceOwner
	// NamespacedResource
}

type K8sNode struct {
	K8sEntity
	ResourceOwner
}

func (n K8sNode) GetId() string {
	return fmt.Sprintf("node/%s", n.GetName())
}

type Asset interface {
}

// JWT RFC: https://datatracker.ietf.org/doc/html/rfc7519#section-4
type JWToken struct {
	Subject        string   `json:"sub"`
	Audience       []string `json:"aud"`
	Issuer         string   `json:"iss"`
	ExpiresAt      int      `json:"exp"`
	IssuedAt       int      `json:"iat"`
	NotValidBefore int      `json:"nbf"`
	Raw            string
}

type ServiceAccountToken struct {
	JWToken
	Kubernetes struct {
		Namespace string `json:"namespace"`
		Pod       struct {
			Name string `json:"name"`
			UID  string `json:"uid"`
		} `json:"pod"`
		ServiceAccount struct {
			Name string `json:"name"`
			UID  string `json:"uid"`
		} `json:"serviceaccount"`
		Warnafter int `json:"warnafter"`
	} `json:"kubernetes.io"`
	// TODO verify if issuer is indicator of K8s API server?
	// PodUid             string
	Raw string
}

// TODO differentiate between k8s resources and a IAM entity?
type ServiceAccount struct {
	K8sEntity
	// kind: str = "ServiceAccount"
	Token ServiceAccountToken
	// token: str | ServiceAccountToken | None = Field(None, exclude=True)
	Can []string
}

func (sa ServiceAccount) GetId() string {
	return fmt.Sprintf("ns/%s/sa/%s", sa.GetNamespace(), sa.GetName())
}

func (sa ServiceAccount) GetKind() string {
	return "ServiceAccount"
}
