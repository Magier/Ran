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

type Requirement interface {
	Satisfies(any) bool
}

type IdentityType string
type AccessLevel struct {
	user  int // 0 = none, 1 = user, 2 = root
	level int // 0 = none, 1 = read, 2 = exec
}

func (lvl AccessLevel) Satisfies(value any) bool {
	if v, ok := value.(AccessLevel); ok {
		return v.user <= lvl.user && v.level <= lvl.level
	}
	return false
}

var (
	NoAccess = AccessLevel{user: 0, level: 0}
	UserRead = AccessLevel{user: 1, level: 1}
	UserExec = AccessLevel{user: 1, level: 2}
	RootRead = AccessLevel{user: 2, level: 1}
	RootExec = AccessLevel{user: 2, level: 2}
)

const (
	AdminUser      IdentityType = "AdminUser"
	User           IdentityType = "User"
	ServiceAccount IdentityType = "ServiceAccount"
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

type Asset interface {
}

type Ownable interface {
	GetId() string
	GetOwner() (OwnerRef, bool)
	SetOwner(name, kind string) Ownable
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
func (e K8sEntity) SetOwner(name, kind string) Ownable {
	e.Owner = OwnerRef{
		Name: name,
		Kind: kind,
	}
	return e
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

func (ns Namespace) GetId() string {
	return "ns/" + ns.Name
}

func (ns Namespace) GetName() string {
	return ns.Name
}

func (ns Namespace) GetKind() string {
	return "Namespace"
}

type Identity struct {
	Name     string
	Kind     IdentityType
	CertData []byte
	KeyData  []byte
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
