package campaign

import (
	"testing"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/domain"
)

func TestAddPodWhenItsWorkloadIsAlreadyKnownDoesNotGenerateExtraWorkload(t *testing.T) {
	a := armory.Armory{}
	c := NewCampaign(&a)
	ns := "default"
	name := "test-pod"
	wl := domain.NewDeployment(name, ns)
	c.AddEntities(wl)
	prevCount := len(c.GetEntities())

	p := domain.Pod{
		K8sEntity: domain.K8sEntity{
			Name:      name + "-123",
			Namespace: ns,
			Owner: domain.OwnerRef{
				Name: name,
				Kind: wl.GetKind(),
			},
		},
	}
	c.AddEntities(p)
	entities := c.GetEntities()
	if len(entities) != prevCount+1 {
		t.Errorf("Only the new pod should've been added")
	}
}
