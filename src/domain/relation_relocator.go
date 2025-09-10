package domain

// ID function for any end type.
type IDFunc[T any] func(T) string

// Your existing Entity, Relation, RelationImpl assumed present.

type RelationRelocator interface {
	Relation
	// ends as Entities
	// GetSource() Entity
	// GetTarget() Entity

	// copy-style modifiers that return the SAME concrete type R
	WithSource(Entity) Relation
	WithTarget(Entity) Relation
	// WithEnds(Entity, Entity) R
}
