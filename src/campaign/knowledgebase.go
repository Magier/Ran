package campaign

import (
	"fmt"
	"log/slog"
	"regexp"
	"strings"

	"github.com/Magier/Ran/domain"
	"github.com/dominikbraun/graph"
	"github.com/google/uuid"
)

var WorkloadNamePattern = regexp.MustCompile(`^(?P<workload>.*)-[a-z0-9]{9}-[a-z0-9]{5}$`)

type KnowledgeGraph = graph.Graph[string, domain.Entity]

type KnowledgeBase interface {
	GetEntity(id string) (domain.Entity, bool)
	GetEntities() map[string]domain.Entity
	AddEntity(entity domain.Entity) error
	AddEntities(entities ...domain.Entity) (int, error)
	AddRelation(relation domain.Relation) error
	AddRelations(relations ...domain.Relation) (int, error)
	GetPath(source, target string) ([]domain.Entity, []domain.Relation, error)
	GetIncomingEntities(entity domain.Entity, rel domain.Relation) ([]domain.Entity, error)
}

type BuiltInKnowledgeBase struct {
	graph     graph.Graph[string, domain.Entity]
	Entities  map[string]domain.Entity
	Relations map[string]domain.Relation
}

func entityHash(e domain.Entity) string {
	return e.GetId()
}

func InitGraph() BuiltInKnowledgeBase {
	g := graph.New(entityHash, graph.Directed())

	kg := BuiltInKnowledgeBase{
		graph:     g,
		Entities:  make(map[string]domain.Entity),
		Relations: make(map[string]domain.Relation),
	}

	return kg
}

func updateEntity(entity, other domain.Entity) domain.Entity {
	if ownable, ok := entity.(domain.Ownable); ok {
		hasOwner := false
		ownerRef, _ := ownable.GetOwner()
		if ownerRef.Name == "" {
			prevOwnable := other.(domain.Ownable)
			ownerRef, hasOwner = prevOwnable.GetOwner()
		}

		if hasOwner {
			// omg, it can't possibly be idiomaitic Go to be so fking cumbersome ...
			switch e := entity.(type) {
			case domain.Pod:
				e.Owner = ownerRef
				return e
			}
		}
	}
	return entity
}

func (kg BuiltInKnowledgeBase) AddEntity(entity domain.Entity) error {
	kg.Entities[entity.GetId()] = entity
	return kg.graph.AddVertex(entity)
}

func (kg BuiltInKnowledgeBase) AddEntities(entities ...domain.Entity) (int, error) {
	numChanges := 0
	for _, entity := range entities {
		if prevEntity, exists := kg.GetEntity(entity.GetId()); exists {
			entity = updateEntity(entity, prevEntity)
		}
		_ = kg.AddEntity(entity)
		numChanges++

		otherEntities, relations := extractRelatedEntities(kg.Entities, entity)
		changes, err := kg.AddEntities(otherEntities...)
		if err == nil {
			numChanges += changes
		} else {
			slog.Error(err.Error())
		}

		for _, rel := range relations {
			if ownsRel, ok := rel.(domain.Owns); ok {
				if ownable, ok := entity.(domain.Ownable); ok {
					ownerRef, _ := ownable.GetOwner()

					// no valid owner reference. Check possible owner based on previous relations
					if ownerRef.Name == "" {

					} else if ownerRef.Name != ownsRel.Owner.GetName() {
						// TODO: Kind is empty!! fix it
						ownerRef := ownable.SetOwner(ownsRel.Owner.GetName(), ownerRef.Kind)

						if p, ok := entity.(domain.Pod); ok {
							p.Owner = ownerRef
							kg.Entities[entity.GetId()] = p
						} else {
							slog.Warn(fmt.Sprintf("Can't update owner of %s '%s': missing type check", entity.GetKind(), entity.GetId()))
						}
					}
				}
			}

			err := kg.AddRelation(rel)
			if err != nil {
				if err.Error() == "edge already exists" {
					slog.Debug(fmt.Sprintf("Edge '%s' already exists", domain.GetRelationId(rel)))
				} else {
					slog.Warn(fmt.Sprintf("Failed to insert relationship '%s': %v", domain.GetRelationId(rel), err))
				}
			} else {
				numChanges++
			}
		}
	}
	return numChanges, nil
}

func (kg BuiltInKnowledgeBase) GetEntity(id string) (domain.Entity, bool) {
	e, ok := kg.Entities[id]
	return e, ok
}

func (kg BuiltInKnowledgeBase) GetEntities() map[string]domain.Entity {
	return kg.Entities
}
func (kg BuiltInKnowledgeBase) AddRelation(rel domain.Relation) error {
	kg.Relations[domain.GetRelationId(rel)] = rel
	cost := domain.GetRelationCost(rel)
	return kg.graph.AddEdge(rel.GetSourceId(), rel.GetTargetId(),
		graph.EdgeAttribute("label", rel.GetRelationName()),
		graph.EdgeWeight(cost),
		graph.EdgeData(rel),
	)
}

func (kg BuiltInKnowledgeBase) AddRelations(relations ...domain.Relation) (int, error) {
	numChanges := 0
	for _, rel := range relations {
		err := kg.AddRelation(rel)
		if err == nil {
			numChanges += 1
		}
	}
	return numChanges, nil
}
func (kg BuiltInKnowledgeBase) GetIncomingEntities(entity domain.Entity, rel domain.Relation) ([]domain.Entity, error) {
	incoming := []domain.Entity{}
	for name, edge := range kg.Relations {
		if strings.HasSuffix(name, fmt.Sprintf("-[%s]->%s", rel.GetRelationName(), entity.GetId())) {
			if src, ok := kg.Entities[edge.GetSourceId()]; ok {
				incoming = append(incoming, src)
			}
		}
	}

	return incoming, nil
}

func (kg BuiltInKnowledgeBase) GetPath(source, target string) ([]domain.Entity, []domain.Relation, error) {
	path, err := graph.ShortestPath(kg.graph, source, target)
	var _ = path
	if err != nil {
		return nil, nil, err
	}

	adjMatrix, err := kg.graph.AdjacencyMap()
	if err != nil {
		return nil, nil, err
	}

	nodesOnPath := make([]domain.Entity, 0)
	relations := make([]domain.Relation, 0)

	for i := 0; i < len(path)-1; i++ {
		srcId := path[i]
		targetId := path[i+1]

		if adjMap, ok := adjMatrix[srcId]; ok {
			if edge, ok := adjMap[targetId]; ok {
				rel := edge.Properties.Data.(domain.Relation)
				relations = append(relations, rel)
			}
		}
	}

	return nodesOnPath, relations, nil
}

// func (kg BuiltInKnowledgeBase) AddRelation() error {
// 	kg.Relations = append(kg.Relations, rel)
// }

func extractRelatedEntities(entities map[string]domain.Entity, entity domain.Entity) ([]domain.Entity, []domain.Relation) {
	newEntities := []domain.Entity{}
	relations := []domain.Relation{}

	mapNsRelation := func(e domain.Namespaced) bool {
		if nsName := e.GetNamespace(); nsName != "" {
			nsId := "ns/" + nsName
			ns, ok := entities[nsId]
			if !ok {
				ns = domain.Namespace{Name: nsName}
				newEntities = append(newEntities, ns)
			}
			relations = append(relations, domain.Contains{Container: ns, Object: entity})
			return true
		}
		return false
	}

	mapPod := func(pod domain.Pod) bool {
		wl, rel := getWorkloadFromPod(pod)
		newEntities = append(newEntities, wl)
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

	return newEntities, relations
}

func getWorkloadFromPod(pod domain.Pod) (domain.Workload, domain.Relation) {
	var owner domain.Workload
	resOwner := domain.ResourceOwner{
		Pods: []domain.Pod{pod},
	}

	if ownerRef, ok := pod.GetOwner(); ok {
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

		ownerName := pod.GetName()
		if matches := WorkloadNamePattern.FindStringSubmatch(ownerName); len(matches) > 1 {
			ownerName = matches[WorkloadNamePattern.SubexpIndex("workload")]
		}

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
