package domain

import (
	"net"

	v1 "k8s.io/api/core/v1"
)

type PodInterface interface {
	GetId() string
	GetPodName() string
	GetNamespace() string
	GetLabel(label string) (string, bool)
}

type Namespaced interface {
	GetNamespace() string
}

type Pod struct {
	Id          string
	Name        string
	Namespace   string
	Labels      map[string]string
	Annotations map[string]string
	CreatedAt   string
	Spec        v1.PodSpec
	IP          net.IPAddr
}

func (p Pod) GetId() string {
	return p.Id
}

func (p Pod) GetPodName() string {
	return p.Name
}
func (p Pod) GetNamespace() string {
	return p.Namespace
}
func (p Pod) GetLabel(label string) (string, bool) {
	if p.Labels != nil {
		v, ok := p.Labels[label]
		return v, ok
	}
	return "", false
}

type ApiServer struct {
	Pod
	CAData     []byte
	ExternalIP net.IPAddr
}

type Identity struct {
	Name     string
	CertData []byte
	KeyData  []byte
}
