package campaign

import (
	"log/slog"

	"github.com/Magier/Ran/domain"
	"github.com/google/uuid"
)

func extractRelatedEntities(campaign *Campaign, entity domain.Entity) ([]domain.Entity, []domain.Relation) {
	entities := []domain.Entity{}
	relations := []domain.Relation{}

	mapNsRelation := func(e domain.Namespaced) bool {
		if nsName := e.GetNamespace(); nsName != "" {
			nsId := "ns/" + nsName
			ns, ok := campaign.entities[nsId]
			if !ok {
				ns = domain.Namespace{Name: nsName}
				entities = append(entities, ns)
			}
			relations = append(relations, domain.Contains{Container: ns, Object: entity})
			return true
		}
		return false
	}

	mapPod := func(pod domain.Pod) bool {
		wl, rel := getWorkloadFromPod(pod)
		entities = append(entities, wl)
		relations = append(relations, rel)
		return mapNsRelation(pod)
	}

	switch e := entity.(type) {
	case domain.Pod:
		mapPod(e)
	case domain.ApiServer: // sadly Go has no proper way to deal with type hierarchies, so a dedicated case is necessary ... sigh
		mapPod(e.Pod)
	case domain.Deployment:
		mapNsRelation(e)
	}

	return entities, relations
}

func getWorkloadFromPod(pod domain.Pod) (domain.Workload, domain.Relation) {
	var owner domain.Workload
	ownerName := pod.GetName()
	resOwner := domain.ResourceOwner{
		Pods: []domain.Pod{pod},
	}

	if ownerRef, ok := pod.GetOwner(); ok {
		ownerName = ownerRef.Name

		ownerEntity := domain.K8sEntity{
			Name:      ownerRef.Name,
			Kind:      ownerRef.Kind,
			Namespace: pod.GetNamespace(),
		}

		if ownerRef.Kind == "Deployment" {
			owner = domain.Deployment{
				K8sEntity:     ownerEntity,
				ResourceOwner: resOwner,
			}
		} else if ownerRef.Kind == "StatefulSet" {
			owner = domain.StatefulSet{
				K8sEntity:     ownerEntity,
				ResourceOwner: resOwner,
			}
		} else if ownerRef.Kind == "DaemonSet" {
			owner = domain.DaemonSet{
				K8sEntity:     ownerEntity,
				ResourceOwner: resOwner,
			}
		} else if ownerRef.Kind == "AbstractWorkload" {
			owner = domain.AbstractWorkload{
				K8sEntity:     ownerEntity,
				ResourceOwner: resOwner,
			}
		} else if ownerRef.Kind == "Node" {
			owner = domain.K8sNode{
				K8sEntity:     ownerEntity,
				ResourceOwner: resOwner,
			}
		} else {
			slog.Error("Getting workload from pod not implemented for kind " + ownerRef.Kind + " for pod: " + pod.GetName())
		}
	}

	// A pod is always part of a workload, even a static pod
	if owner == nil {
		id := uuid.New()
		owner = domain.AbstractWorkload{
			K8sEntity: domain.K8sEntity{
				Id:        id.String(),
				Name:      ownerName,
				Kind:      "Workload",
				Namespace: pod.GetNamespace(),
			},
			ResourceOwner: resOwner,
		}
	}
	// TODO for a static pod the owner is actually the Node, think of way to properly model this
	rel := domain.Owns{
		Owner:  owner,
		Object: &pod,
	}
	return owner, rel
}
