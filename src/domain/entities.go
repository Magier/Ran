package domain

import (
	"net"

	v1 "k8s.io/api/core/v1"
)

type Protocol string

const ANY Protocol = "ANY"
const TCP Protocol = "TCP"

// const HTTP Protocol = "HTTP"
// const DNS Protocol = "DNS"
// const UDP Protocol = "UDP"
// const mTLS Protocol = "TLS"

type Listener struct {
	ID         string
	Port       uint
	Protocol   Protocol
	Redirector string
	IP         net.IP
}

type Entity interface {
	GetId() string
	GetName() string
	GetKind() string
}

type Relation interface {
}

type Asset interface {
}

type Ownable interface {
	GetOwner() (OwnerRef, bool)
}

type Namespaced interface {
	GetNamespace() string
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
	Labels      map[string]string
	Annotations map[string]string
	CreatedAt   string
	Owner       OwnerRef
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

type NamespacedResource struct {
	Namespace string
}

func (n NamespacedResource) GetNamespace() string {
	return n.Namespace
}

type ApiServer struct {
	Pod
	CAData     []byte
	ExternalIP net.IPAddr
}

type Identity struct {
	Name     string
	Kind     string
	CertData []byte
	KeyData  []byte
}
type Pod struct {
	K8sEntity
	NamespacedResource
	Spec v1.PodSpec
	IP   net.IPAddr
}

type Deployment struct {
	K8sEntity
	NamespacedResource
}

type ReplicaSet struct {
	K8sEntity
	NamespacedResource
}

func (r ReplicaSet) GetKind() string {
	return "ReplicaSet"
}

type StatefulSet struct {
	K8sEntity
	NamespacedResource
}

func (s StatefulSet) GetKind() string {
	return "StatefulSet"
}
