package domain

import (
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
	GetOwner() (OwnerRef, bool)
}

type Namespaced interface {
	GetNamespace() string
}

type EntityPlaceholder interface {
	IsAbstract() bool
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
}

func (e K8sEntity) GetId() string {
	return e.Id
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
	Id   string
	Name string
}

func (ns Namespace) GetId() string {
	return ns.Id
}

func (ns Namespace) GetName() string {
	return ns.Name
}

func (ns Namespace) GetKind() string {
	return "Namespace"
}

type Identity struct {
	Name     string
	Kind     string
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

type Deployment struct {
	K8sEntity
	// NamespacedResource
	ResourceOwner
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
	// NamespacedResource
}

func (r ReplicaSet) GetKind() string {
	return "ReplicaSet"
}

type StatefulSet struct {
	K8sEntity
	// NamespacedResource
}

func (s StatefulSet) GetKind() string {
	return "StatefulSet"
}
