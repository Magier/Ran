package campaign

import (
	"reflect"
	"testing"

	"github.com/Magier/Ran/domain"
)

func newTestPodWithBinary(name, ns, binary string) domain.Pod {
	pod := domain.NewPod(name, ns)
	if binary != "" {
		pod.SystemImpl.Binaries[binary] = "/usr/local/bin/" + binary
	}
	return pod
}

// identityWithKubeletProxy returns an IdentityProvider with a single identity
// that has GET nodes/proxy permission and a token.
func identityWithKubeletProxy() IdentityProvider {
	ids := map[string]domain.Identity{
		"sa/default/attacker-sa": domain.ServiceAccount{
			K8sEntity: domain.K8sEntity{
				Name:      "attacker-sa",
				Namespace: "default",
				Kind:      "ServiceAccount",
			},
			Token: domain.ServiceAccountToken{Raw: "fake-token"},
			Entitelements: []domain.RBACPermission{
				{Verb: "get", ResourceType: "nodes/proxy"},
			},
		},
	}
	return func() map[string]domain.Identity { return ids }
}

// noIdentities returns an IdentityProvider with no identities.
func noIdentities() IdentityProvider {
	return func() map[string]domain.Identity { return map[string]domain.Identity{} }
}

func TestKubeletExecRule_PodAddedFirst_NodeLater(t *testing.T) {
	kb := InitGraph()
	re := NewRuleEngine(kb, identityWithKubeletProxy(), KubeletExecRule)

	// Add a pod with ran-ws binary
	pod := newTestPodWithBinary("attacker", "default", "ran-ws")
	_ = kb.AddEntity(pod)

	added := domain.Facts{Entities: []domain.Entity{pod}}
	toAdd, toRemove, _ := re.EvaluateDelta(added, domain.Facts{})

	// No nodes yet → no relations
	if len(toAdd) != 0 {
		t.Errorf("Expected no relations (no nodes yet), got %d: %v", len(toAdd), toAdd)
	}
	if len(toRemove) != 0 {
		t.Errorf("Expected no removals, got %d", len(toRemove))
	}

	// Now add a node
	node := domain.NewK8sNode("worker-1")
	_ = kb.AddEntity(node)

	added = domain.Facts{Entities: []domain.Entity{node}}
	toAdd, toRemove, _ = re.EvaluateDelta(added, domain.Facts{})

	// KubeletExec should now be created
	if len(toAdd) != 1 {
		t.Fatalf("Expected 1 relation, got %d: %v", len(toAdd), toAdd)
	}
	if toAdd[0].GetRelationName() != "kubelet-exec" {
		t.Errorf("Expected kubelet-exec relation, got %s", toAdd[0].GetRelationName())
	}
	if toAdd[0].GetSourceId() != pod.GetId() {
		t.Errorf("Expected source %s, got %s", pod.GetId(), toAdd[0].GetSourceId())
	}
	if toAdd[0].GetTargetId() != node.GetId() {
		t.Errorf("Expected target %s, got %s", node.GetId(), toAdd[0].GetTargetId())
	}
	if len(toRemove) != 0 {
		t.Errorf("Expected no removals, got %d", len(toRemove))
	}
}

func TestKubeletExecRule_NodeAddedFirst_PodLater(t *testing.T) {
	kb := InitGraph()
	re := NewRuleEngine(kb, identityWithKubeletProxy(), KubeletExecRule)

	// Add node first
	node := domain.NewK8sNode("worker-1")
	_ = kb.AddEntity(node)
	added := domain.Facts{Entities: []domain.Entity{node}}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})

	if len(toAdd) != 0 {
		t.Errorf("Expected no relations (no pods yet), got %d", len(toAdd))
	}

	// Add pod with binary
	pod := newTestPodWithBinary("attacker", "default", "ran-ws")
	_ = kb.AddEntity(pod)
	added = domain.Facts{Entities: []domain.Entity{pod}}
	toAdd, _, _ = re.EvaluateDelta(added, domain.Facts{})

	if len(toAdd) != 1 {
		t.Fatalf("Expected 1 kubelet-exec relation, got %d", len(toAdd))
	}
	if toAdd[0].GetRelationName() != "kubelet-exec" {
		t.Errorf("Expected kubelet-exec, got %s", toAdd[0].GetRelationName())
	}
}

func TestKubeletExecRule_PodWithoutBinary_NoRelation(t *testing.T) {
	kb := InitGraph()
	re := NewRuleEngine(kb, identityWithKubeletProxy(), KubeletExecRule)

	node := domain.NewK8sNode("worker-1")
	_ = kb.AddEntity(node)
	re.IndexEntity(node)

	// Pod WITHOUT ran-ws binary
	pod := newTestPodWithBinary("innocent", "default", "")
	_ = kb.AddEntity(pod)

	added := domain.Facts{Entities: []domain.Entity{pod}}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})

	if len(toAdd) != 0 {
		t.Errorf("Expected no relations (no ran-ws binary), got %d", len(toAdd))
	}
}

func TestKubeletExecRule_NoIdentityWithPermission_NoRelation(t *testing.T) {
	kb := InitGraph()
	re := NewRuleEngine(kb, noIdentities(), KubeletExecRule)

	pod := newTestPodWithBinary("attacker", "default", "ran-ws")
	node := domain.NewK8sNode("worker-1")
	_ = kb.AddEntity(pod)
	_ = kb.AddEntity(node)

	added := domain.Facts{Entities: []domain.Entity{pod, node}}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})

	if len(toAdd) != 0 {
		t.Errorf("Expected no relations (no identity with GET nodes/proxy), got %d", len(toAdd))
	}
}

func TestKubeletExecRule_IdentityWithoutToken_NoRelation(t *testing.T) {
	kb := InitGraph()
	// Identity has the permission but no token
	ids := map[string]domain.Identity{
		"sa/default/no-token-sa": domain.ServiceAccount{
			K8sEntity: domain.K8sEntity{
				Name:      "no-token-sa",
				Namespace: "default",
				Kind:      "ServiceAccount",
			},
			// No token set
			Entitelements: []domain.RBACPermission{
				{Verb: "get", ResourceType: "nodes/proxy"},
			},
		},
	}
	re := NewRuleEngine(kb, func() map[string]domain.Identity { return ids }, KubeletExecRule)

	pod := newTestPodWithBinary("attacker", "default", "ran-ws")
	node := domain.NewK8sNode("worker-1")
	_ = kb.AddEntity(pod)
	_ = kb.AddEntity(node)

	added := domain.Facts{Entities: []domain.Entity{pod, node}}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})

	if len(toAdd) != 0 {
		t.Errorf("Expected no relations (identity has no token), got %d", len(toAdd))
	}
}

func TestKubeletExecRule_MultipleNodes(t *testing.T) {
	kb := InitGraph()
	re := NewRuleEngine(kb, identityWithKubeletProxy(), KubeletExecRule)

	// Add 3 nodes
	nodes := []domain.K8sNode{
		domain.NewK8sNode("worker-1"),
		domain.NewK8sNode("worker-2"),
		domain.NewK8sNode("worker-3"),
	}
	for _, n := range nodes {
		_ = kb.AddEntity(n)
		re.IndexEntity(n)
	}

	// Add pod with binary
	pod := newTestPodWithBinary("attacker", "default", "ran-ws")
	_ = kb.AddEntity(pod)

	added := domain.Facts{Entities: []domain.Entity{pod}}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})

	if len(toAdd) != 3 {
		t.Errorf("Expected 3 kubelet-exec relations (one per node), got %d", len(toAdd))
	}
}

func TestKubeletExecRule_CanReachTrigger_RemovesOnUnreachable(t *testing.T) {
	kb := InitGraph()
	re := NewRuleEngine(kb, identityWithKubeletProxy(), KubeletExecRule)

	pod := newTestPodWithBinary("attacker", "default", "ran-ws")
	node := domain.NewK8sNode("worker-1")
	_ = kb.AddEntity(pod)
	_ = kb.AddEntity(node)

	// Initial: both added, should create kubelet-exec
	added := domain.Facts{Entities: []domain.Entity{pod, node}}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})
	if len(toAdd) != 1 {
		t.Fatalf("Expected 1 kubelet-exec, got %d", len(toAdd))
	}

	// Simulate adding the relation to KB
	_ = kb.AddRelation(toAdd[0])

	// Now, simulate "can-reach" being added (should re-evaluate but relation already exists)
	canReach := domain.CanReach{SourceId: pod.GetId(), TargetId: node.GetId()}
	_ = kb.AddRelation(canReach)

	added = domain.Facts{Relations: []domain.Relation{canReach}}
	toAdd, toRemove, _ := re.EvaluateDelta(added, domain.Facts{})

	// Relation already exists and condition is still true → no new add, no remove
	if len(toAdd) != 0 {
		t.Errorf("Expected no new relations (already exists), got %d", len(toAdd))
	}
	if len(toRemove) != 0 {
		t.Errorf("Expected no removals, got %d", len(toRemove))
	}
}

func TestKubeletExecRule_EntityRemoved_CleansUpRelation(t *testing.T) {
	kb := InitGraph()
	re := NewRuleEngine(kb, identityWithKubeletProxy(), KubeletExecRule)

	pod := newTestPodWithBinary("attacker", "default", "ran-ws")
	node := domain.NewK8sNode("worker-1")
	_ = kb.AddEntity(pod)
	_ = kb.AddEntity(node)

	// Create the relation
	added := domain.Facts{Entities: []domain.Entity{pod, node}}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})
	if len(toAdd) != 1 {
		t.Fatalf("Expected 1 kubelet-exec, got %d", len(toAdd))
	}
	_ = kb.AddRelation(toAdd[0])

	// Remove the node
	removed := domain.Facts{Entities: []domain.Entity{node}}
	_, toRemove, _ := re.EvaluateDelta(domain.Facts{}, removed)

	if len(toRemove) != 1 {
		t.Errorf("Expected 1 relation removed on node removal, got %d", len(toRemove))
	}
}

func TestRuleEngine_Reset(t *testing.T) {
	kb := InitGraph()
	re := NewRuleEngine(kb, identityWithKubeletProxy(), KubeletExecRule)

	pod := newTestPodWithBinary("attacker", "default", "ran-ws")
	node := domain.NewK8sNode("worker-1")
	_ = kb.AddEntity(pod)
	_ = kb.AddEntity(node)

	added := domain.Facts{Entities: []domain.Entity{pod, node}}
	_, _, _ = re.EvaluateDelta(added, domain.Facts{})

	re.Reset()

	if len(re.typeIndex) != 0 {
		t.Errorf("Expected empty type index after reset, got %d entries", len(re.typeIndex))
	}
	if len(re.managedRelations) != 0 {
		t.Errorf("Expected empty managed relations after reset, got %d entries", len(re.managedRelations))
	}
}

func TestCustomRule_SimpleMatch(t *testing.T) {
	kb := InitGraph()

	customRule := Rule{
		Name:       "TestRule",
		SourceType: reflect.TypeOf(domain.Pod{}),
		TargetType: reflect.TypeOf(domain.Pod{}),
		Match: func(source, target domain.Entity, re *RuleEngine) ConditionState {
			s := source.(domain.Pod)
			tgt := target.(domain.Pod)
			if s.GetNamespace() == tgt.GetNamespace() {
				return ConditionTrue
			}
			return ConditionFalse
		},
		Build: func(source, target domain.Entity, re *RuleEngine) []domain.Relation {
			return []domain.Relation{
				domain.CanReach{SourceId: source.GetId(), TargetId: target.GetId()},
			}
		},
	}

	re := NewRuleEngine(kb, noIdentities(), customRule)

	pod1 := newTestPodWithBinary("pod1", "default", "")
	pod2 := newTestPodWithBinary("pod2", "default", "")
	pod3 := newTestPodWithBinary("pod3", "kube-system", "")
	_ = kb.AddEntity(pod1)
	_ = kb.AddEntity(pod2)

	// Add pod1 and pod2 in the same namespace
	added := domain.Facts{Entities: []domain.Entity{pod1, pod2}}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})

	// pod1→pod2 and pod2→pod1
	if len(toAdd) != 2 {
		t.Errorf("Expected 2 relations for same-namespace pods, got %d", len(toAdd))
	}

	// Add pod3 in different namespace
	_ = kb.AddEntity(pod3)
	added = domain.Facts{Entities: []domain.Entity{pod3}}
	toAdd, _, _ = re.EvaluateDelta(added, domain.Facts{})

	// pod3 is in a different namespace, so no new relations
	if len(toAdd) != 0 {
		t.Errorf("Expected 0 relations for different-namespace pod, got %d", len(toAdd))
	}
}

// --- KubeletPodExec chained rule tests ---

// setupKubeletPodExecEngine creates a rule engine with both KubeletExec and KubeletPodExec rules.
func setupKubeletPodExecEngine() (*BuiltInKnowledgeBase, *RuleEngine) {
	kb := InitGraph()
	re := NewRuleEngine(kb, identityWithKubeletProxy(), KubeletExecRule, KubeletPodExecRule)
	return kb, re
}

func TestKubeletPodExecRule_KubeletExecTriggersFanOut(t *testing.T) {
	kb, re := setupKubeletPodExecEngine()

	node := domain.NewK8sNode("worker-1")
	victim1 := domain.NewPod("victim-1", "default")
	victim2 := domain.NewPod("victim-2", "default")
	attacker := newTestPodWithBinary("attacker", "default", "ran-ws")

	// Add all entities
	for _, e := range []domain.Entity{node, victim1, victim2, attacker} {
		_ = kb.AddEntity(e)
	}

	// Establish runs-on relations (victims run on the node)
	runsOn1 := domain.RunsOn{Pod: victim1, Node: node}
	runsOn2 := domain.RunsOn{Pod: victim2, Node: node}
	_ = kb.AddRelation(runsOn1)
	_ = kb.AddRelation(runsOn2)

	// Step 1: Add all entities — should create KubeletExec (attacker→node)
	added := domain.Facts{
		Entities:  []domain.Entity{node, victim1, victim2, attacker},
		Relations: []domain.Relation{runsOn1, runsOn2},
	}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})

	// Should have KubeletExec(attacker→node)
	var kubeletExecFound bool
	for _, rel := range toAdd {
		if rel.GetRelationName() == "kubelet-exec" {
			kubeletExecFound = true
			_ = kb.AddRelation(rel)
		}
	}
	if !kubeletExecFound {
		t.Fatal("Expected KubeletExec relation to be created")
	}

	// Step 2: Feed the KubeletExec relation as a trigger for the next pass
	triggerDelta := domain.Facts{Relations: toAdd}
	toAdd2, _, _ := re.EvaluateDelta(triggerDelta, domain.Facts{})

	// Should have KubeletPodExec(node→victim1) and KubeletPodExec(node→victim2)
	podExecCount := 0
	for _, rel := range toAdd2 {
		if rel.GetRelationName() == "kubelet-pod-exec" {
			podExecCount++
			if rel.GetSourceId() != node.GetId() {
				t.Errorf("Expected source to be node, got %s", rel.GetSourceId())
			}
		}
	}
	if podExecCount != 2 {
		t.Errorf("Expected 2 kubelet-pod-exec relations, got %d", podExecCount)
	}
}

func TestKubeletPodExecRule_NewPodOnNodeWithKubeletExec(t *testing.T) {
	kb, re := setupKubeletPodExecEngine()

	node := domain.NewK8sNode("worker-1")
	attacker := newTestPodWithBinary("attacker", "default", "ran-ws")
	_ = kb.AddEntity(node)
	_ = kb.AddEntity(attacker)

	// Create KubeletExec first
	added := domain.Facts{Entities: []domain.Entity{node, attacker}}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})
	for _, rel := range toAdd {
		_ = kb.AddRelation(rel)
	}
	// Feed the kubelet-exec trigger
	triggerDelta := domain.Facts{Relations: toAdd}
	_, _, _ = re.EvaluateDelta(triggerDelta, domain.Facts{}) // no pods on node yet, nothing to create

	// Now add a new pod that runs on the node
	newPod := domain.NewPod("new-victim", "default")
	_ = kb.AddEntity(newPod)
	runsOn := domain.RunsOn{Pod: newPod, Node: node}
	_ = kb.AddRelation(runsOn)

	added = domain.Facts{
		Entities:  []domain.Entity{newPod},
		Relations: []domain.Relation{runsOn},
	}
	toAdd, _, _ = re.EvaluateDelta(added, domain.Facts{})

	// The runs-on trigger should cause KubeletPodExec(node→newPod) to be created
	var found bool
	for _, rel := range toAdd {
		if rel.GetRelationName() == "kubelet-pod-exec" && rel.GetTargetId() == newPod.GetId() {
			found = true
		}
	}
	if !found {
		t.Errorf("Expected kubelet-pod-exec for new pod, got %v", toAdd)
	}
}

func TestKubeletPodExecRule_NoKubeletExec_NoRelation(t *testing.T) {
	kb, re := setupKubeletPodExecEngine()

	node := domain.NewK8sNode("worker-1")
	pod := domain.NewPod("victim", "default")
	_ = kb.AddEntity(node)
	_ = kb.AddEntity(pod)

	runsOn := domain.RunsOn{Pod: pod, Node: node}
	_ = kb.AddRelation(runsOn)

	added := domain.Facts{
		Entities:  []domain.Entity{node, pod},
		Relations: []domain.Relation{runsOn},
	}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})

	// No KubeletExec on this node, so no KubeletPodExec
	for _, rel := range toAdd {
		if rel.GetRelationName() == "kubelet-pod-exec" {
			t.Error("Did not expect kubelet-pod-exec without kubelet-exec on node")
		}
	}
}

func TestKubeletPodExecRule_PodNotOnNode_NoRelation(t *testing.T) {
	kb, re := setupKubeletPodExecEngine()

	node1 := domain.NewK8sNode("worker-1")
	node2 := domain.NewK8sNode("worker-2")
	attacker := newTestPodWithBinary("attacker", "default", "ran-ws")
	pod := domain.NewPod("victim", "default")

	for _, e := range []domain.Entity{node1, node2, attacker, pod} {
		_ = kb.AddEntity(e)
	}

	// victim runs on node2 (no kubelet-exec will target node2 since we'll only add kubelet-exec for node1)
	runsOn := domain.RunsOn{Pod: pod, Node: node2}
	_ = kb.AddRelation(runsOn)

	// Manually add KubeletExec only for node1 (not node2)
	kubeletExec := domain.KubeletExec{Pod: attacker, Node: node1}
	_ = kb.AddRelation(kubeletExec)
	re.IndexEntity(node1)
	re.IndexEntity(node2)
	re.IndexEntity(attacker)
	re.IndexEntity(pod)

	// Feed kubelet-exec trigger
	triggerDelta := domain.Facts{Relations: []domain.Relation{kubeletExec, runsOn}}
	toAdd, _, _ := re.EvaluateDelta(triggerDelta, domain.Facts{})

	// node2 has no incoming kubelet-exec → no kubelet-pod-exec for victim on node2
	for _, rel := range toAdd {
		if rel.GetRelationName() == "kubelet-pod-exec" && rel.GetTargetId() == pod.GetId() && rel.GetSourceId() == node2.GetId() {
			t.Error("Did not expect kubelet-pod-exec for pod on node without kubelet-exec")
		}
	}
}

func TestKubeletPodExecRule_NoCycleWithAttackerPod(t *testing.T) {
	kb, re := setupKubeletPodExecEngine()

	node := domain.NewK8sNode("worker-1")
	attacker := newTestPodWithBinary("attacker", "default", "ran-ws")

	_ = kb.AddEntity(node)
	_ = kb.AddEntity(attacker)

	// Attacker runs on the same node it has kubelet-exec to
	runsOn := domain.RunsOn{Pod: attacker, Node: node}
	_ = kb.AddRelation(runsOn)

	// Step 1: create KubeletExec(attacker→node)
	added := domain.Facts{
		Entities:  []domain.Entity{node, attacker},
		Relations: []domain.Relation{runsOn},
	}
	toAdd, _, _ := re.EvaluateDelta(added, domain.Facts{})
	for _, rel := range toAdd {
		_ = kb.AddRelation(rel)
	}

	// Step 2: trigger chained rules
	triggerDelta := domain.Facts{Relations: toAdd}
	toAdd2, _, _ := re.EvaluateDelta(triggerDelta, domain.Facts{})

	// Should NOT create KubeletPodExec(node→attacker) since attacker already has KubeletExec→node
	for _, rel := range toAdd2 {
		if rel.GetRelationName() == "kubelet-pod-exec" && rel.GetTargetId() == attacker.GetId() {
			t.Error("Did not expect kubelet-pod-exec back to the attacker pod (would create a cycle)")
		}
	}
}
