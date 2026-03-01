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

// KubeletPodExecRule creates KubeletPodExec relations (Node → Pod) for every pod
// running on a node that has an incoming KubeletExec relation.
// Triggers:
//   - "kubelet-exec": when a KubeletExec is added/removed, fan out to all pods on that node
//   - "runs-on": when a pod is placed on a node, check if that node has KubeletExec
var KubeletPodExecRule = Rule{
	Name:       "KubeletPodExec",
	SourceType: reflect.TypeOf(domain.K8sNode{}),
	TargetType: reflect.TypeOf(domain.Pod{}),
	Match: func(source, target domain.Entity, re *RuleEngine) ConditionState {
		node := source.(domain.K8sNode)
		pod := target.(domain.Pod)

		// Required: node has at least one incoming kubelet-exec relation
		if !hasIncomingKubeletExec(re.kb, node) {
			return ConditionFalse
		}

		// Required: pod runs on this node (runs-on relation exists)
		if !podRunsOnNode(re.kb, pod, node) {
			return ConditionFalse
		}

		// Exclude: don't create Node→Pod if this pod already has KubeletExec→Node (avoid cycles)
		if hasKubeletExecToNode(re.kb, pod, node) {
			return ConditionFalse
		}

		return ConditionTrue
	},
	Build: func(source, target domain.Entity) []domain.Relation {
		return []domain.Relation{
			domain.KubeletPodExec{
				Node: source.(domain.K8sNode),
				Pod:  target.(domain.Pod),
			},
		}
	},
	Apply: func(source, target domain.Entity) []domain.Entity {
		pod := target.(domain.Pod)
		if pod.SystemImpl != nil {
			pod.AccessLevel = domain.UserExec
			return []domain.Entity{pod}
		}
		return nil
	},
	RelationTriggers: []string{"kubelet-exec", "runs-on"},
}

// hasIncomingKubeletExec checks if any kubelet-exec relation targets this node.
func hasIncomingKubeletExec(kb KnowledgeBase, node domain.K8sNode) bool {
	for _, rel := range kb.GetIncomingEdges(node) {
		if rel.GetRelationName() == "kubelet-exec" {
			return true
		}
	}
	return false
}

// hasKubeletExecToNode checks if a kubelet-exec relation exists from pod to node.
func hasKubeletExecToNode(kb KnowledgeBase, pod domain.Pod, node domain.K8sNode) bool {
	kubeletExecId := domain.GetRelationId(domain.KubeletExec{Pod: pod, Node: node})
	_, exists := kb.GetRelations()[kubeletExecId]
	return exists
}

// podRunsOnNode checks if a runs-on relation exists from pod to node.
func podRunsOnNode(kb KnowledgeBase, pod domain.Pod, node domain.K8sNode) bool {
	runsOnId := domain.GetRelationId(domain.RunsOn{Pod: pod, Node: node})
	_, exists := kb.GetRelations()[runsOnId]
	return exists
}

// DefaultRules returns the standard set of rules for the rule engine.
func DefaultRules() []Rule {
	return []Rule{
		KubeletExecRule,
		KubeletPodExecRule,
	}
}
