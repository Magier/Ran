package explorer

import (
	"testing"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
)

func TestAddEntity(t *testing.T) {
	t.Run("add Pod before its workload", func(t *testing.T) {
		wl := domain.NewDeployment("test-workload", "default")
		pod := domain.NewPod(wl.Name+"-1", wl.Namespace)
		pod.Owner = domain.OwnerRef{Name: wl.Name, Kind: wl.Kind}

		root := newTree()
		a := armory.Armory{}
		c := campaign.NewCampaign(a)
		// add the pod first
		addEntity(root, c, pod, nil)
		addEntity(root, c, wl, nil)

		if len(root.children.children) != 1 {
			t.Error("Root node must have a child node for the namespace")
		}
		nsNode := root.children.At(0)
		if nsNode.Children().Length() != 1 {
			t.Error("Only 1 workload node should be in the namespace")
		}
		wlNode := nsNode.Children().At(0)
		if wlNode.Children().Length() != 1 {
			t.Error("Exactly 1 Pod node should be in the workload")
		}
	})

	t.Run("add Pod after its workload", func(t *testing.T) {
		wl := domain.NewDeployment("test-workload", "default")
		pod := domain.NewPod(wl.Name+"-1", wl.Namespace)
		pod.Owner = domain.OwnerRef{Name: wl.Name, Kind: wl.Kind}

		root := newTree()
		a := armory.Armory{}
		c := campaign.NewCampaign(a)
		// add the workload first
		addEntity(root, c, wl, nil)
		addEntity(root, c, pod, nil)

		if len(root.children.children) != 1 {
			t.Error("Root node must have a child node for the namespace")
		}
		nsNode := root.children.At(0)
		if nsNode.Children().Length() != 1 {
			t.Error("Only 1 workload node should be in the namespace")
		}
		wlNode := nsNode.Children().At(0)
		if wlNode.Children().Length() != 1 {
			t.Error("Exactly 1 Pod node should be in the workload")
		}
	})
}
