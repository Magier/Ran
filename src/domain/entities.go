package domain

import v1 "k8s.io/api/core/v1"

type Pod struct {
	Name        string
	Namespace   string
	Labels      map[string]string
	Annotations map[string]string
	CreatedAt   string
	Spec        v1.PodSpec
}
