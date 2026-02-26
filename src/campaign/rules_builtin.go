package campaign

import (
	"reflect"

	"github.com/Magier/Ran/domain"
)

// KubeletExecRule creates KubeletExec relations between pods (with ran-ws binary)
// and nodes, when:
//  1. Required: pod has the ran-ws binary
//  2. Required: any known identity has GET nodes/proxy permission (with a token Ran can use)
//  3. Optional (open-world): pod can reach the node (unknown = assume reachable)
var KubeletExecRule = Rule{
	Name:       "KubeletExec",
	SourceType: reflect.TypeOf(domain.Pod{}),
	TargetType: reflect.TypeOf(domain.K8sNode{}),
	Match: func(source, target domain.Entity, re *RuleEngine) ConditionState {
		pod, ok := source.(domain.Pod)
		if !ok || pod.SystemImpl == nil {
			return ConditionFalse
		}

		// Required: pod has the ran-ws websocket binary
		if _, hasRanWs := pod.Binaries["ran-ws"]; !hasRanWs {
			return ConditionFalse
		}

		// Required: any identity with GET nodes/proxy permission and a usable token
		if !hasKubeletProxyIdentity(re) {
			return ConditionFalse
		}

		// Optional (open-world): can pod reach node?
		reachability := checkReachability(re.kb, pod, target)
		if reachability == ConditionFalse {
			return ConditionFalse
		}

		return ConditionTrue
	},
	Build: func(source, target domain.Entity) []domain.Relation {
		return []domain.Relation{
			domain.KubeletExec{
				Pod:  source.(domain.Pod),
				Node: target.(domain.K8sNode),
			},
		}
	},
	RelationTriggers: []string{"can-reach"},
}

// hasKubeletProxyIdentity checks if any known identity has GET nodes/proxy
// permission and a token that Ran can use.
func hasKubeletProxyIdentity(re *RuleEngine) bool {
	if re.Identities == nil {
		return false
	}
	for _, identity := range re.Identities() {
		if identity.GetToken() == "" {
			continue
		}
		if _, ok := identity.Can("get", "nodes/proxy"); ok {
			return true
		}
	}
	return false
}

// checkReachability returns the reachability state between two entities.
// - If a "can-reach" relation exists from source to target → ConditionTrue
// - If no relation exists → ConditionUnknown (open-world assumption)
// - Future: if a "cannot-reach" relation exists → ConditionFalse
func checkReachability(kb KnowledgeBase, source, target domain.Entity) ConditionState {
	rels := kb.GetRelations()

	canReachId := domain.GetRelationId(domain.CanReach{
		SourceId: source.GetId(),
		TargetId: target.GetId(),
	})
	if _, exists := rels[canReachId]; exists {
		return ConditionTrue
	}

	return ConditionUnknown
}

// DefaultRules returns the standard set of rules for the rule engine.
func DefaultRules() []Rule {
	return []Rule{
		KubeletExecRule,
	}
}
