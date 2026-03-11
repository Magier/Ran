package campaign

import (
	"reflect"

	"github.com/Magier/Ran/domain"
)

// KubeletExecRule creates KubeletExecSource relations between pods and nodes, when:
//  1. Required: pod has a binary matching any of the KubeletExecProcedures
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

		// Required: pod has a binary for at least one kubelet exec procedure
		if _, ok := findMatchingKubeletProcedure(re, pod); !ok {
			return ConditionFalse
		}

		// Required: any identity with GET nodes/proxy permission and a usable token
		if _, ok := getKubeletProxyIdentity(re); !ok {
			return ConditionFalse
		}

		// Optional (open-world): can pod reach node?
		reachability := checkReachability(re.kb, pod, target)
		if reachability == ConditionFalse {
			return ConditionFalse
		}

		return ConditionTrue
	},
	Build: func(source, target domain.Entity, re *RuleEngine) []domain.Relation {
		identity, _ := getKubeletProxyIdentity(re)
		proc, _ := findMatchingKubeletProcedure(re, source.(domain.Pod))
		return []domain.Relation{
			&domain.KubeletExecSource{
				Pod:       source.(domain.Pod),
				Node:      target.(domain.K8sNode),
				Identity:  identity,
				Procedure: proc,
			},
		}
	},
	RelationTriggers: []string{"can-reach"},
}

// findMatchingKubeletProcedure returns the first procedure whose tool is available on the pod.
func findMatchingKubeletProcedure(re *RuleEngine, pod domain.Pod) (domain.Procedure, bool) {
	for _, proc := range re.Procedures {
		if pod.HasBinary(proc.GetTool()).Bool() {
			return proc, true
		}
	}
	return domain.Procedure{}, false
}

// getKubeletProxyIdentity checks if any known identity has GET nodes/proxy
// permission and a token that Ran can use.
func getKubeletProxyIdentity(re *RuleEngine) (domain.Identity, bool) {
	if re.Identities == nil {
		return nil, false
	}
	for _, identity := range re.Identities() {
		if identity.GetToken() == "" {
			continue
		}
		if _, ok := identity.Can("get", "nodes/proxy"); ok {
			return identity, true
		}
	}
	return nil, false
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

// KubeletPodExecRule creates KubeletExecSink relations (Node → Pod) for every pod
// running on a node that has an incoming KubeletExecSource relation.
// Triggers:
//   - "kubelet-exec": when a KubeletExecSource is added/removed, fan out to all pods on that node
//   - "runs-on": when a pod is placed on a node, check if that node has KubeletExecSource
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

		// Exclude: don't create Node→Pod if this pod already has KubeletExecSource→Node (avoid cycles)
		if hasKubeletExecToNode(re.kb, pod, node) {
			return ConditionFalse
		}

		return ConditionTrue
	},
	Build: func(source, target domain.Entity, re *RuleEngine) []domain.Relation {
		// Propagate the procedure from the KubeletExecSource relation on this node
		proc := getKubeletExecProcedure(re.kb, source.(domain.K8sNode))
		return []domain.Relation{
			&domain.KubeletExecSink{
				Node:      source.(domain.K8sNode),
				Pod:       target.(domain.Pod),
				Procedure: proc,
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

// getKubeletExecProcedure extracts the Procedure from the KubeletExecSource relation targeting this node.
func getKubeletExecProcedure(kb KnowledgeBase, node domain.K8sNode) domain.Procedure {
	for _, rel := range kb.GetIncomingEdges(node) {
		if src, ok := rel.(*domain.KubeletExecSource); ok {
			return src.Procedure
		}
	}
	return domain.Procedure{}
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
	kubeletExecId := domain.GetRelationId(&domain.KubeletExecSource{Pod: pod, Node: node})
	_, exists := kb.GetRelations()[kubeletExecId]
	return exists
}

// podRunsOnNode checks if a runs-on relation exists from pod to node.
func podRunsOnNode(kb KnowledgeBase, pod domain.Pod, node domain.K8sNode) bool {
	runsOnId := domain.GetRelationId(domain.RunsOn{Pod: pod, Node: node})
	_, exists := kb.GetRelations()[runsOnId]
	return exists
}

// CanExecAccessRule updates the AccessLevel of any system (Pod, K8sNode, or UnknownSystem)
// that has an incoming CanExecChannel relation. This is a pure entity-update rule that
// doesn't create new relations.
var CanExecAccessRule = Rule{
	Name:       "CanExecAccess",
	SourceType: reflect.TypeOf(domain.Pod{}), // Dummy - any source
	TargetType: reflect.TypeOf(domain.Pod{}), // Will match Pods
	Match: func(source, target domain.Entity, re *RuleEngine) ConditionState {
		// Check if target is a System with incoming can-exec relation
		sys, isSystem := target.(domain.System)
		if !isSystem {
			return ConditionFalse
		}

		// Already has UserExec? No update needed
		if sys.GetAccessLevel() == domain.UserExec {
			return ConditionFalse
		}

		// Check for incoming can-exec relation
		for _, rel := range re.kb.GetIncomingEdges(target) {
			if rel.GetRelationName() == "can-exec" {
				return ConditionTrue
			}
		}
		return ConditionFalse
	},
	Build: func(source, target domain.Entity, re *RuleEngine) []domain.Relation {
		// Entity-only side effect rule - no relations created
		return nil
	},
	Apply: func(source, target domain.Entity) []domain.Entity {
		// Update AccessLevel based on entity type
		switch sys := target.(type) {
		case domain.Pod:
			if sys.SystemImpl != nil {
				sys.AccessLevel = domain.UserExec
				return []domain.Entity{sys}
			}
		case domain.K8sNode:
			if sys.SystemImpl != nil {
				sys.AccessLevel = domain.UserExec
				return []domain.Entity{sys}
			}
		case domain.UnknownSystem:
			if sys.SystemImpl != nil {
				sys.AccessLevel = domain.UserExec
				return []domain.Entity{sys}
			}
		}
		return nil
	},
	RelationTriggers: []string{"can-exec"},
}

// Cross-type rules for other system type combinations targeting Pods
var CanExecAccessNodeToPodRule = Rule{
	Name:             "CanExecAccessNodeToPod",
	SourceType:       reflect.TypeOf(domain.K8sNode{}),
	TargetType:       reflect.TypeOf(domain.Pod{}),
	Match:            CanExecAccessRule.Match,
	Build:            CanExecAccessRule.Build,
	Apply:            CanExecAccessRule.Apply,
	RelationTriggers: []string{"can-exec"},
}

var CanExecAccessUnknownToPodRule = Rule{
	Name:             "CanExecAccessUnknownToPod",
	SourceType:       reflect.TypeOf(domain.UnknownSystem{}),
	TargetType:       reflect.TypeOf(domain.Pod{}),
	Match:            CanExecAccessRule.Match,
	Build:            CanExecAccessRule.Build,
	Apply:            CanExecAccessRule.Apply,
	RelationTriggers: []string{"can-exec"},
}

// Cross-type rules for other system type combinations targeting K8sNodes
var CanExecAccessNodeRule = Rule{
	Name:             "CanExecAccessNode",
	SourceType:       reflect.TypeOf(domain.K8sNode{}),
	TargetType:       reflect.TypeOf(domain.K8sNode{}),
	Match:            CanExecAccessRule.Match,
	Build:            CanExecAccessRule.Build,
	Apply:            CanExecAccessRule.Apply,
	RelationTriggers: []string{"can-exec"},
}

var CanExecAccessPodToNodeRule = Rule{
	Name:             "CanExecAccessPodToNode",
	SourceType:       reflect.TypeOf(domain.Pod{}),
	TargetType:       reflect.TypeOf(domain.K8sNode{}),
	Match:            CanExecAccessRule.Match,
	Build:            CanExecAccessRule.Build,
	Apply:            CanExecAccessRule.Apply,
	RelationTriggers: []string{"can-exec"},
}

var CanExecAccessUnknownToNodeRule = Rule{
	Name:             "CanExecAccessUnknownToNode",
	SourceType:       reflect.TypeOf(domain.UnknownSystem{}),
	TargetType:       reflect.TypeOf(domain.K8sNode{}),
	Match:            CanExecAccessRule.Match,
	Build:            CanExecAccessRule.Build,
	Apply:            CanExecAccessRule.Apply,
	RelationTriggers: []string{"can-exec"},
}

// Cross-type rules for other system type combinations targeting UnknownSystems
var CanExecAccessUnknownSystemRule = Rule{
	Name:             "CanExecAccessUnknownSystem",
	SourceType:       reflect.TypeOf(domain.UnknownSystem{}),
	TargetType:       reflect.TypeOf(domain.UnknownSystem{}),
	Match:            CanExecAccessRule.Match,
	Build:            CanExecAccessRule.Build,
	Apply:            CanExecAccessRule.Apply,
	RelationTriggers: []string{"can-exec"},
}

var CanExecAccessPodToUnknownRule = Rule{
	Name:             "CanExecAccessPodToUnknown",
	SourceType:       reflect.TypeOf(domain.Pod{}),
	TargetType:       reflect.TypeOf(domain.UnknownSystem{}),
	Match:            CanExecAccessRule.Match,
	Build:            CanExecAccessRule.Build,
	Apply:            CanExecAccessRule.Apply,
	RelationTriggers: []string{"can-exec"},
}

var CanExecAccessNodeToUnknownRule = Rule{
	Name:             "CanExecAccessNodeToUnknown",
	SourceType:       reflect.TypeOf(domain.K8sNode{}),
	TargetType:       reflect.TypeOf(domain.UnknownSystem{}),
	Match:            CanExecAccessRule.Match,
	Build:            CanExecAccessRule.Build,
	Apply:            CanExecAccessRule.Apply,
	RelationTriggers: []string{"can-exec"},
}

// PropagateHostIPRule propagates the HostIP from a Pod to the K8sNode it runs on.
// When a Pod has a RunsOn relation to a K8sNode and the Pod has a HostIP filled,
// this rule adds that IP to the node's IPs list if it's not already there.
var PropagateHostIPRule = Rule{
	Name:       "PropagateHostIP",
	SourceType: reflect.TypeOf(domain.Pod{}),
	TargetType: reflect.TypeOf(domain.K8sNode{}),
	Match: func(source, target domain.Entity, re *RuleEngine) ConditionState {
		pod, ok := source.(domain.Pod)
		if !ok {
			return ConditionFalse
		}

		node, ok := target.(domain.K8sNode)
		if !ok || node.SystemImpl == nil {
			return ConditionFalse
		}

		// Required: pod has a runs-on relation to this node
		if !podRunsOnNode(re.kb, pod, node) {
			return ConditionFalse
		}

		// Required: pod has a non-empty HostIP
		if len(pod.HostIP.IP) == 0 {
			return ConditionFalse
		}

		// Check if the node already has this IP
		for _, ip := range node.IPs {
			if ip.IP.Equal(pod.HostIP.IP) {
				return ConditionFalse // IP already present, no update needed
			}
		}

		return ConditionTrue
	},
	Build: func(source, target domain.Entity, re *RuleEngine) []domain.Relation {
		// Entity-only side effect rule - no relations created
		return nil
	},
	Apply: func(source, target domain.Entity) []domain.Entity {
		pod := source.(domain.Pod)
		node := target.(domain.K8sNode)

		// Add the pod's HostIP to the node's IPs
		if node.SystemImpl != nil {
			node.IPs = append(node.IPs, pod.HostIP)
			return []domain.Entity{node}
		}
		return nil
	},
	RelationTriggers: []string{"runs-on"},
}

// DefaultRules returns the standard set of rules for the rule engine.
func DefaultRules() []Rule {
	return []Rule{
		KubeletExecRule,
		KubeletPodExecRule,
		CanExecAccessRule,
		CanExecAccessNodeToPodRule,
		CanExecAccessUnknownToPodRule,
		CanExecAccessNodeRule,
		CanExecAccessPodToNodeRule,
		CanExecAccessUnknownToNodeRule,
		CanExecAccessUnknownSystemRule,
		CanExecAccessPodToUnknownRule,
		CanExecAccessNodeToUnknownRule,
		PropagateHostIPRule,
	}
}
