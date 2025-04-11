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
