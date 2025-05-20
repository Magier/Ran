package campaign

import (
	"testing"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/domain"
)

func TestAddSinglePod(t *testing.T) {
	p := domain.NewPod("test", "default")

	a := armory.Armory{}
	c := NewCampaign(&a)
	c.AddEntities(p)

	pods := c.GetPods()
	if len(pods) != 1 {
		t.Error("Expect exactly 1 pod in the knowledge base")
	}
}

func TestDontAddExtraWorkloadWhenAddingPodWithOwner(t *testing.T) {
	nsName := "default"
	depl := domain.NewDeployment("test", nsName)
	p := domain.NewPod("test", nsName)
	p.Owner = domain.OwnerRef{
		Kind: depl.Kind,
		Name: depl.Name,
	}

	a := armory.Armory{}
	c := NewCampaign(&a)
	c.AddEntities(p) // implicitely adds Deployment because of the OwnerRef
	pods := c.GetEntities()
	if len(pods) != 3 { // NS + Deployment + Pod
		t.Error("The pod has exactly 1 owner!")
	}

	// adding it again should make no difference
	p = domain.NewPod("test", nsName)
	c.AddEntities(depl, p)
	if len(c.GetEntities()) != 3 { // NS + Deployment + Pod
		t.Error("The pod has exactly 1 owner!")
	}
}

func TestMoreInformationOnPodOwner(t *testing.T) {
	nsName := "default"
	p := domain.NewPod("test", nsName)
	a := armory.Armory{}
	c := NewCampaign(&a)

	// this will add the intermediary AbstractWorkload
	c.AddEntities(p)

	depl := domain.NewDeployment("test", nsName)
	c.AddEntities(depl)
	owns := domain.Owns{Owner: depl, Object: p}
	c.AddRelations(owns)

	pods := c.GetEntities()
	if len(pods) != 3 {
		t.Error("The pod has exactly 1 owner!")
	}
}
func TestRemoveEntity_RemovesEntityAndRelations(t *testing.T) {
	// Setup
	kg := InitGraph()
	ns := domain.Namespace{Name: "default"}
	pod := domain.NewPod("testpod", "default")

	// Add namespace and pod, which should also create a Contains relation
	_, err := kg.AddEntities(ns, pod)
	if err != nil {
		t.Fatalf("unexpected error adding entities: %v", err)
	}

	// Confirm both entities exist
	if _, ok := kg.GetEntity(ns.GetId()); !ok {
		t.Fatal("namespace not found after adding")
	}
	if _, ok := kg.GetEntity(pod.GetId()); !ok {
		t.Fatal("pod not found after adding")
	}

	// Confirm relation exists
	foundRelation := false
	for _, rel := range kg.GetRelations() {
		if rel.GetSourceId() == ns.GetId() && rel.GetTargetId() == pod.GetId() {
			foundRelation = true
			break
		}
	}
	if !foundRelation {
		t.Error("expected relation between namespace and pod not found")
	}

	// Remove pod
	err = kg.RemoveEntity(pod)
	if err != nil {
		t.Fatalf("unexpected error removing pod: %v", err)
	}

	// Pod should be gone
	if _, ok := kg.GetEntity(pod.GetId()); ok {
		t.Error("pod still present after removal")
	}

	// Relation should be gone
	for _, rel := range kg.GetRelations() {
		if rel.GetSourceId() == ns.GetId() && rel.GetTargetId() == pod.GetId() {
			t.Error("relation still present after pod removal")
		}
	}

	// Namespace should still exist
	if _, ok := kg.GetEntity(ns.GetId()); !ok {
		t.Error("namespace missing after pod removal")
	}
}
