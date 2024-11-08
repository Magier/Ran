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

type IdentityType string

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

func (p Pod) GetId() string {
	return fmt.Sprintf("ns/%s/pod/%s", p.GetNamespace(), p.GetName())
}

type Deployment struct {
	K8sEntity
	// NamespacedResource
	ResourceOwner
}

func (d Deployment) GetId() string {
	return fmt.Sprintf("ns/%s/depl/%s", d.GetNamespace(), d.GetName())
}

type Service struct {
	K8sEntity
	// NamespacedResource
	Targets []string
	Host    string
	FQDN    string
	Ports   map[string]int
}

func (s Service) GetId() string {
	return fmt.Sprintf("ns/%s/svc/%s", s.GetNamespace(), s.GetName())
}

type ReplicaSet struct {
	K8sEntity
	ResourceOwner
	// NamespacedResource
}

func (s ReplicaSet) GetId() string {
	return fmt.Sprintf("ns/%s/rs/%s", s.GetNamespace(), s.GetName())
}

type StatefulSet struct {
	K8sEntity
	ResourceOwner
	// NamespacedResource
}

func (s StatefulSet) GetId() string {
	return fmt.Sprintf("ns/%s/sts/%s", s.GetNamespace(), s.GetName())
}

type DaemonSet struct {
	K8sEntity
	ResourceOwner
	// NamespacedResource
}

func (s DaemonSet) GetId() string {
	return fmt.Sprintf("ns/%s/ds/%s", s.GetNamespace(), s.GetName())
}

type K8sNode struct {
	K8sEntity
	ResourceOwner
}

func (n K8sNode) GetId() string {
	return fmt.Sprintf("node/%s", n.GetName())
}
