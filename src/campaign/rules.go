package campaign

import (
	"fmt"
	"log/slog"
	"reflect"

	"github.com/Magier/Ran/domain"
)

// ConditionState represents tri-state logic for rule evaluation.
// Unknown means the condition hasn't been determined yet — treated as satisfied (open-world assumption).
type ConditionState int

const (
	ConditionUnknown ConditionState = iota // no information → assume satisfied
	ConditionTrue                          // definitely satisfied
	ConditionFalse                         // definitely NOT satisfied → blocks the rule
)

// Rule declares how to produce implied relations from entity pairs.
// When an entity matching SourceType or TargetType is added/updated/removed,
// the engine evaluates the rule for all matching counterpart entities.
type Rule struct {
	Name string

	// SourceType and TargetType define which entity types participate.
	// Uses reflect.Type for precise Go type matching (not GetKind() strings,
	// which can collide across different struct types).
	SourceType reflect.Type
	TargetType reflect.Type

	// Match evaluates whether the rule's conditions are met for a (source, target) pair.
	// Returns ConditionFalse to block relation creation/keep.
	// Returns ConditionTrue or ConditionUnknown to allow it.
	// The RuleEngine is passed so rules can access the KB, identities, and other context.
	Match func(source, target domain.Entity, re *RuleEngine) ConditionState

	// Build creates the relation(s) to add when Match passes.
	// The RuleEngine is passed so rules can access context (e.g., identities).
	Build func(source, target domain.Entity, re *RuleEngine) []domain.Relation

	// Apply returns entity updates to apply when the rule fires (optional).
	// Use this when a rule needs to mutate entities (e.g., set AccessLevel)
	// in addition to creating relations.
	Apply func(source, target domain.Entity) []domain.Entity

	// RelationTriggers are relation names (e.g. "can-reach") that, when added or removed,
	// should cause this rule to re-evaluate the affected (source, target) pair.
	RelationTriggers []string
}

// IdentityProvider returns the current set of known identities.
// This bridges the gap until identities are unified into the KB graph.
type IdentityProvider func() map[string]domain.Identity

// RuleEngine manages declared rules and evaluates them incrementally against KB changes.
type RuleEngine struct {
	rules []Rule
	kb    KnowledgeBase

	// typeIndex maps reflect.Type → set of entity IDs of that type.
	// Populated as entities are added/removed.
	typeIndex map[reflect.Type]map[string]bool

	// managedRelations tracks which relation IDs were produced by which rule,
	// so we can garbage-collect them when conditions change.
	// key: relation ID (from domain.GetRelationId), value: rule name
	managedRelations map[string]string

	// Identities provides access to RBAC identities (until they're graph entities).
	Identities IdentityProvider
}

func NewRuleEngine(kb KnowledgeBase, identities IdentityProvider, rules ...Rule) *RuleEngine {
	return &RuleEngine{
		rules:            rules,
		kb:               kb,
		typeIndex:        make(map[reflect.Type]map[string]bool),
		managedRelations: make(map[string]string),
		Identities:       identities,
	}
}

// entityType returns the reflect.Type of an entity, dereferencing pointers.
func entityType(e domain.Entity) reflect.Type {
	t := reflect.TypeOf(e)
	if t.Kind() == reflect.Ptr {
		t = t.Elem()
	}
	return t
}

// IndexEntity adds an entity to the type index. Call this when an entity is added to the KB.
func (re *RuleEngine) IndexEntity(e domain.Entity) {
	t := entityType(e)
	if re.typeIndex[t] == nil {
		re.typeIndex[t] = make(map[string]bool)
	}
	re.typeIndex[t][e.GetId()] = true
}

// UnindexEntity removes an entity from the type index. Call this when an entity is removed from the KB.
func (re *RuleEngine) UnindexEntity(e domain.Entity) {
	t := entityType(e)
	if idx, ok := re.typeIndex[t]; ok {
		delete(idx, e.GetId())
	}
}

// getEntitiesOfType returns all entity IDs indexed under the given type.
func (re *RuleEngine) getEntitiesOfType(t reflect.Type) []string {
	ids := make([]string, 0, len(re.typeIndex[t]))
	for id := range re.typeIndex[t] {
		ids = append(ids, id)
	}
	return ids
}

// EvaluateDelta processes added/removed/updated entities and relations,
// returning new implied relations to add, stale ones to remove, and entity updates to apply.
func (re *RuleEngine) EvaluateDelta(added, removed domain.Facts) (toAdd []domain.Relation, toRemove []domain.Relation, entityUpdates []domain.Entity) {
	// 1. Remove index entries for removed entities and clean up their managed relations
	for _, e := range removed.Entities {
		re.cleanupEntityRelations(e, &toRemove)
		re.UnindexEntity(e)
	}

	// 2. Index new entities
	for _, e := range added.Entities {
		re.IndexEntity(e)
	}

	// 3. Evaluate rules for added/updated entities (new entities + entities that may have changed)
	evaluated := make(map[string]bool) // avoid duplicate evaluations: "ruleName:srcId:tgtId"
	for _, e := range added.Entities {
		re.evaluateEntityAgainstRules(e, evaluated, &toAdd, &toRemove, &entityUpdates)
	}

	// 4. Evaluate relation triggers (e.g., a CanReach was added/removed)
	for _, rel := range added.Relations {
		re.evaluateRelationTrigger(rel, evaluated, &toAdd, &toRemove, &entityUpdates)
	}
	for _, rel := range removed.Relations {
		re.evaluateRelationTrigger(rel, evaluated, &toAdd, &toRemove, &entityUpdates)
	}

	return toAdd, toRemove, entityUpdates
}

// evaluateEntityAgainstRules checks all rules where the entity matches either SourceType or TargetType.
func (re *RuleEngine) evaluateEntityAgainstRules(
	e domain.Entity,
	evaluated map[string]bool,
	toAdd *[]domain.Relation,
	toRemove *[]domain.Relation,
	entityUpdates *[]domain.Entity,
) {
	eType := entityType(e)

	for _, rule := range re.rules {
		if eType == rule.SourceType {
			// Entity is a source — check against all targets of the matching type
			for _, targetId := range re.getEntitiesOfType(rule.TargetType) {
				if targetId == e.GetId() {
					continue // skip self-pairing
				}
				key := fmt.Sprintf("%s:%s:%s", rule.Name, e.GetId(), targetId)
				if evaluated[key] {
					continue
				}
				evaluated[key] = true

				target, ok := re.kb.GetEntity(targetId)
				if !ok {
					continue
				}
				re.evaluatePair(rule, e, target, toAdd, toRemove, entityUpdates)
			}
		}

		if eType == rule.TargetType {
			// Entity is a target — check against all sources of the matching type
			for _, sourceId := range re.getEntitiesOfType(rule.SourceType) {
				if sourceId == e.GetId() {
					continue
				}
				key := fmt.Sprintf("%s:%s:%s", rule.Name, sourceId, e.GetId())
				if evaluated[key] {
					continue
				}
				evaluated[key] = true

				source, ok := re.kb.GetEntity(sourceId)
				if !ok {
					continue
				}
				re.evaluatePair(rule, source, e, toAdd, toRemove, entityUpdates)
			}
		}
	}
}

// evaluateRelationTrigger handles relation-based triggers (e.g., CanReach added/removed).
// When a trigger fires, each endpoint that matches SourceType or TargetType is
// evaluated against ALL counterpart entities — not just the other endpoint.
// This is necessary for chained rules (e.g., KubeletPodExec depends on KubeletExec,
// where the relation's endpoints don't directly map to the rule's source/target).
func (re *RuleEngine) evaluateRelationTrigger(
	rel domain.Relation,
	evaluated map[string]bool,
	toAdd *[]domain.Relation,
	toRemove *[]domain.Relation,
	entityUpdates *[]domain.Entity,
) {
	relName := rel.GetRelationName()
	srcId := rel.GetSourceId()
	tgtId := rel.GetTargetId()

	for _, rule := range re.rules {
		for _, trigger := range rule.RelationTriggers {
			if trigger != relName {
				continue
			}

			// For each endpoint of the relation, if it matches one side of the rule,
			// fan out and evaluate it against ALL entities of the other side.
			endpoints := []string{srcId, tgtId}
			for _, eid := range endpoints {
				entity, ok := re.kb.GetEntity(eid)
				if !ok {
					continue
				}
				re.evaluateEntityAgainstRule(rule, entity, evaluated, toAdd, toRemove, entityUpdates)
			}
		}
	}
}

// evaluateEntityAgainstRule checks a single rule for a given entity,
// pairing it with all counterpart entities of the matching type.
func (re *RuleEngine) evaluateEntityAgainstRule(
	rule Rule,
	e domain.Entity,
	evaluated map[string]bool,
	toAdd *[]domain.Relation,
	toRemove *[]domain.Relation,
	entityUpdates *[]domain.Entity,
) {
	eType := entityType(e)

	if eType == rule.SourceType {
		for _, targetId := range re.getEntitiesOfType(rule.TargetType) {
			if targetId == e.GetId() {
				continue
			}
			key := fmt.Sprintf("%s:%s:%s", rule.Name, e.GetId(), targetId)
			if evaluated[key] {
				continue
			}
			evaluated[key] = true
			target, ok := re.kb.GetEntity(targetId)
			if !ok {
				continue
			}
			re.evaluatePair(rule, e, target, toAdd, toRemove, entityUpdates)
		}
	}

	if eType == rule.TargetType {
		for _, sourceId := range re.getEntitiesOfType(rule.SourceType) {
			if sourceId == e.GetId() {
				continue
			}
			key := fmt.Sprintf("%s:%s:%s", rule.Name, sourceId, e.GetId())
			if evaluated[key] {
				continue
			}
			evaluated[key] = true
			source, ok := re.kb.GetEntity(sourceId)
			if !ok {
				continue
			}
			re.evaluatePair(rule, source, e, toAdd, toRemove, entityUpdates)
		}
	}
}

// evaluatePair evaluates a single rule for a (source, target) pair and decides
// whether to add or remove the implied relation.
func (re *RuleEngine) evaluatePair(
	rule Rule,
	source, target domain.Entity,
	toAdd *[]domain.Relation,
	toRemove *[]domain.Relation,
	entityUpdates *[]domain.Entity,
) {
	result := rule.Match(source, target, re)

	// Build the relations to figure out their IDs
	rels := rule.Build(source, target, re)

	switch result {
	case ConditionTrue, ConditionUnknown:
		newRelation := false
		for _, rel := range rels {
			relId := domain.GetRelationId(rel)
			if _, alreadyManaged := re.managedRelations[relId]; !alreadyManaged {
				// Check if the relation already exists in the KB
				existingRels := re.kb.GetRelations()
				if _, exists := existingRels[relId]; !exists {
					*toAdd = append(*toAdd, rel)
					re.managedRelations[relId] = rule.Name
					newRelation = true
					slog.Debug("Rule engine: adding relation", "rule", rule.Name, "relation", relId)
				} else {
					// Already in KB, just track it
					re.managedRelations[relId] = rule.Name
				}
			}
		}

		// Apply entity updates when a new relation is produced
		if newRelation && rule.Apply != nil {
			updates := rule.Apply(source, target)
			*entityUpdates = append(*entityUpdates, updates...)
		}

	case ConditionFalse:
		for _, rel := range rels {
			relId := domain.GetRelationId(rel)
			if _, managed := re.managedRelations[relId]; managed {
				*toRemove = append(*toRemove, rel)
				delete(re.managedRelations, relId)
				slog.Debug("Rule engine: removing relation", "rule", rule.Name, "relation", relId)
			}
		}
	}
}

// cleanupEntityRelations removes all managed relations involving the given entity.
func (re *RuleEngine) cleanupEntityRelations(e domain.Entity, toRemove *[]domain.Relation) {
	eId := e.GetId()
	for relId, ruleName := range re.managedRelations {
		// Parse the relation ID to check if it involves this entity.
		// Relation ID format: "sourceId-[relName]->targetId"
		if containsEntityId(relId, eId) {
			// Look up the actual relation from KB to remove it properly
			rels := re.kb.GetRelations()
			if rel, ok := rels[relId]; ok {
				*toRemove = append(*toRemove, rel)
			}
			delete(re.managedRelations, relId)
			slog.Debug("Rule engine: cleanup relation for removed entity", "rule", ruleName, "relation", relId, "entity", eId)
		}
	}
}

// containsEntityId checks if a relation ID string references the given entity ID.
func containsEntityId(relId, entityId string) bool {
	// Relation ID format: "sourceId-[relName]->targetId"
	// An entity ID appears either before "-[" or after "]->"
	return len(relId) > 0 &&
		(relId[:min(len(entityId), len(relId))] == entityId ||
			len(relId) > len(entityId) && relId[len(relId)-len(entityId):] == entityId)
}

// Reset clears all state (for campaign reset).
func (re *RuleEngine) Reset() {
	re.typeIndex = make(map[reflect.Type]map[string]bool)
	re.managedRelations = make(map[string]string)
}
