package domain

import "log/slog"

type Relation interface {
	GetSource() string
	GetTarget() string
	GetRelationName() string
}

type Reference struct {
	Source string
	Target string
	Medium string
}

func (r Reference) GetSource() string {
	return r.Source
}
func (r Reference) GetTarget() string {
	return r.Target
}

func (r Reference) GetRelationName() string {
	return "references"
}

func (r Reference) String() string {
	return "extracted from " + r.Medium
}

type Contains struct {
	Container Entity
	Object    Entity
}

func (r Contains) GetSource() string {
	return r.Container.GetName()
}

func (r Contains) GetTarget() string {
	return r.Object.GetName()
}

func (r Contains) GetRelationName() string {
	return "contains"
}

type Owns struct {
	Owner  Entity
	Object Ownable
}

func (r Owns) GetSource() string {
	return r.Owner.GetName()
}
func (r Owns) GetTarget() string {
	if o, ok := r.Object.(K8sEntity); ok {
		return o.GetName()
	}
	slog.Error("Owns target is not a K8sEntity!", "target", r.Object)
	return "?"
}

func (r Owns) GetRelationName() string {
	return "owns"
}
