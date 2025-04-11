package campaign

import (
	"fmt"
	"testing"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/domain"
)

func TestAddAbstractWorkloadWhenAddingPod(t *testing.T) {
	a := armory.Armory{}
	c := NewCampaign(&a)
	ns := "default"
	name := "test-pod"
	p := domain.Pod{
		K8sEntity: domain.K8sEntity{
			Name:      name,
			Namespace: ns,
		},
	}

	c.AddEntities(p)

	entities := c.GetEntities()
	if len(entities) < 2 {
		t.Errorf("there should be the pod and at least the generated AbstractWorkload")
	}

	e, ok := entities[fmt.Sprintf("ns/%s/wl/%s", ns, name)]
	if !ok {
		t.Errorf("No entitiy with the expected id found")
	}

	wl, ok := e.(domain.Workload)
	if !ok {
		t.Errorf("Second entity must be a valid Workload")
	}

	if len(wl.GetPods()) != 1 {
		t.Errorf("Workload must have the pod as owned pod")
	}
}

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

func TestReplaceAbstractWorkloadWhenMoreConcreteTypeIsKnown(t *testing.T) {
	t.Fail()
}

func TestAddingSameEntityWithAdditionalInformationMergesThem(t *testing.T) {
	t.Fail()
}
