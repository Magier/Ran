package campaign

import (
	"fmt"
	"log/slog"
	"regexp"
	"strings"

	"github.com/Magier/Ran/domain"
	"github.com/dominikbraun/graph"
)

var WorkloadNamePattern = regexp.MustCompile(`^(?P<workload>.*)-[a-z0-9]{9}-[a-z0-9]{5}$`)

type KnowledgeGraph = graph.Graph[string, domain.Entity]
type AdjacencyList = map[string]map[string]string

type Path struct {
	Nodes     []domain.Entity
	Relations []domain.Relation
}

type KnowledgeBase interface {
	GetEntity(id string) (domain.Entity, bool)
	GetC2s() []domain.C2System
	GetEntities() map[string]domain.Entity
	GetClusters() map[string]domain.Entity
	AddEntity(entity domain.Entity) error
	RemoveEntity(entity domain.Entity) error
	AddEntities(entities ...domain.Entity) (int, error)
	RemoveEntities(entities ...domain.Entity) (int, error)
	AddRelation(relation domain.Relation) error
	RemoveRelation(relation domain.Relation) error
	AddRelations(relations ...domain.Relation) (int, error)
	RemoveRelations(relations ...domain.Relation) (int, error)
	GetRelations() map[string]domain.Relation
	GetPath(source, target string) (Path, error)
	GetAllPaths(source, target string) ([]Path, error)
	GetIncomingEntities(entity domain.Entity, rel domain.Relation) ([]domain.Entity, error)
	GetAdjecencyList() AdjacencyList
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

func (kg BuiltInKnowledgeBase) AddEntity(entity domain.Entity) error {
	kg.Entities[entity.GetId()] = entity
	return kg.graph.AddVertex(entity)
}

func (kg BuiltInKnowledgeBase) RemoveEntity(entity domain.Entity) error {
	e, ok := kg.Entities[entity.GetId()]
	if !ok {
		return fmt.Errorf("entity %s not found", entity.GetId())
	}

	delete(kg.Entities, e.GetId())
	err := kg.DisconnectNode(e.GetId())
	if err != nil {
		return err
	}

	return kg.graph.RemoveVertex(e.GetId())
}

func (kg BuiltInKnowledgeBase) DisconnectNode(nodeID string) error {
	for _, relation := range kg.Relations {
		if relation.GetSourceId() == nodeID || relation.GetTargetId() == nodeID {
			if err := kg.RemoveRelation(relation); err != nil {
				return err
			}
		}
	}
	return nil
}

func (kg BuiltInKnowledgeBase) GetClusters() map[string]domain.Entity {
	clusters := make(map[string]domain.Entity)
	if cluster, ok := kg.Entities[domain.TheOnlyClusterId]; ok {
		clusters[domain.TheOnlyClusterId] = cluster.(domain.Cluster)
	}
	return clusters
}

func (kg BuiltInKnowledgeBase) GetCluster() (domain.Cluster, bool) {
	if cluster, ok := kg.Entities[domain.TheOnlyClusterId]; ok {
		return cluster.(domain.Cluster), true
	}
	return domain.Cluster{}, false
}

func (kg BuiltInKnowledgeBase) GetAdjecencyList() AdjacencyList {
	m, err := kg.graph.AdjacencyMap()
	if err != nil {
		slog.Error(err.Error())
		return nil
	}

	adjList := make(map[string]map[string]string)
	for src, targets := range m {
		adjList[src] = make(map[string]string)
		for target, edge := range targets {
			adjList[src][target] = edge.Properties.Attributes["label"]
		}
	}

	return adjList
}

func (kg BuiltInKnowledgeBase) AddEntities(entities ...domain.Entity) (int, error) {
	numChanges := 0
	cluster, hasCluster := kg.GetCluster()

	for _, entity := range entities {
		if prevEntity, exists := kg.GetEntity(entity.GetId()); exists {
			entity = domain.UpdateEntity(entity, prevEntity)
			_ = kg.AddEntity(entity)
		} else {
			_ = kg.AddEntity(entity) // entity has to be added before the relation
			switch e := entity.(type) {
			case domain.Namespace:
				if hasCluster {
					err := kg.AddRelation(domain.Contains{Container: cluster, Object: entity})
					if err != nil {
						slog.Error(err.Error())
					}
				}
			case domain.K8sNode:
				if hasCluster {
					err := kg.AddRelation(domain.ManagesNode{Cluster: cluster, Node: e})
					if err != nil {
						slog.Error(err.Error())
					}
				}
			}
		}

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
					if ownerRef.Name == "" || ownerRef.Name != ownsRel.Owner.GetName() {
						ownerRef = ownable.SetOwner(ownsRel.Owner.GetName(), ownsRel.Owner.GetKind())
						if p, ok := entity.(domain.Pod); ok {
							p.Owner = ownerRef
							kg.Entities[entity.GetId()] = p
						} else if apiServer, ok := entity.(domain.ApiServer); ok {
							apiServer.Owner = ownerRef
							kg.Entities[entity.GetId()] = apiServer
						} else {
							slog.Warn(fmt.Sprintf("Can't update owner of %s '%s': missing type check", entity.GetKind(), entity.GetId()))
						}
					}
				}
			}

			err := kg.AddRelation(rel)
			if err != nil {
				if err.Error() != "edge already exists" {
					slog.Warn(fmt.Sprintf("Failed to insert relationship '%s': %v", domain.GetRelationId(rel), err))
				}
			} else {
				numChanges++
			}
		}
	}
	return numChanges, nil
}

func (kg BuiltInKnowledgeBase) RemoveEntities(entities ...domain.Entity) (int, error) {
	numChanges := 0

	for _, entitiy := range entities {
		if _, ok := kg.Entities[entitiy.GetId()]; ok {
			// remove all relations to this entity
			for _, rel := range kg.Relations {
				if rel.GetSourceId() == entitiy.GetId() || rel.GetTargetId() == entitiy.GetId() {
					err := kg.RemoveRelation(rel)
					if err != nil {
						slog.Error(err.Error())
					}
				}
			}
			// remove the entity from the graph
			err := kg.RemoveEntity(entitiy)
			if err != nil {
				return numChanges, err
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

func (kg BuiltInKnowledgeBase) GetRelations() map[string]domain.Relation {
	return kg.Relations
}

func (kg BuiltInKnowledgeBase) GetC2(name string) (domain.C2System, bool) {
	for _, entity := range kg.Entities {
		if c2, ok := entity.(domain.C2System); ok {
			if c2.Name == name {
				return c2, true
			}
		}
	}
	return domain.C2System{}, false
}
func (kg BuiltInKnowledgeBase) GetC2s() []domain.C2System {
	c2s := make([]domain.C2System, 0)
	for _, entity := range kg.GetEntities() {
		if c2, ok := entity.(domain.C2System); ok {
			c2s = append(c2s, c2)
		}
	}
	return c2s
}

func (kg BuiltInKnowledgeBase) AddRelation(rel domain.Relation) error {
	kg.Relations[domain.GetRelationId(rel)] = rel
	cost := domain.GetRelationCost(rel)

	dir := "forward"
	if rel.IsReverse() {
		dir = "back"
	}
	return kg.graph.AddEdge(rel.GetSourceId(), rel.GetTargetId(),
		graph.EdgeAttribute("label", rel.GetRelationName()),
		graph.EdgeAttribute("dir", dir),
		graph.EdgeWeight(cost),
		graph.EdgeData(rel),
	)
}
func (kg BuiltInKnowledgeBase) RemoveRelation(relation domain.Relation) error {
	delete(kg.Relations, domain.GetRelationId(relation))
	return kg.graph.RemoveEdge(relation.GetSourceId(), relation.GetTargetId())
}

func (kg BuiltInKnowledgeBase) AddRelations(relations ...domain.Relation) (int, error) {
	numChanges := 0
	for _, rel := range relations {
		err := kg.AddRelation(rel)
		if err == nil {
			numChanges += 1
		} else if err.Error() == "edge already exists" {
			paths, err := kg.GetPath(rel.GetSourceId(), rel.GetTargetId()) // ensure the edge is in the graph
			if err != nil {
				slog.Warn(fmt.Sprintf("~~~ Failed to get path for relation %s: %v", domain.GetRelationId(rel), err))
			}
			// prevEdge := kg.Relations[domain.GetRelationId(rel)]
			prevEdge := paths.Relations[len(paths.Relations)-1] // the last relation in the path is the one we want to update
			prevCost := domain.GetRelationCost(prevEdge)        // ensure the cost is set
			newCost := domain.GetRelationCost(rel)
			if prevCost > newCost {
				slog.Warn(fmt.Sprintf("Updating edge %s -> %s with new cost %d (was %d)", domain.GetRelationId(prevEdge), domain.GetRelationId(rel), newCost, prevCost))
				err = kg.RemoveRelation(prevEdge)
				if err != nil {
					slog.Warn(fmt.Sprintf("Failed to remove previous relation %s: %v", domain.GetRelationId(prevEdge), err))
				}
				err = kg.AddRelation(rel) // re-add the relation with the new cost
				if err == nil {
					numChanges += 1
				}
			}
		}
	}
	return numChanges, nil
}

func (kg BuiltInKnowledgeBase) RemoveRelations(relations ...domain.Relation) (int, error) {
	numChanges := 0
	for _, rel := range relations {
		err := kg.RemoveRelation(rel)
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

func (kg BuiltInKnowledgeBase) GetAllPaths(source, target string) ([]Path, error) {
	paths, err := graph.AllPathsBetween(kg.graph, source, target)
	if err != nil {
		return nil, err
	}
	allPaths := make([]Path, 0, len(paths))

	for _, path := range paths {
		resolvedPath, err := resolvePath(kg, path, source)
		if err != nil {
			return nil, fmt.Errorf("failed to resolve path %v: %w", path, err)
		}
		allPaths = append(allPaths, resolvedPath)
	}
	return allPaths, nil
}

func resolvePath(kg BuiltInKnowledgeBase, path []string, source string) (Path, error) {
	adjMatrix, err := kg.graph.AdjacencyMap()
	if err != nil {
		return Path{}, err
	}

	nodesOnPath := make([]domain.Entity, 0)
	nodesOnPath = append(nodesOnPath, kg.Entities[source])
	relations := make([]domain.Relation, 0)

	for i := 0; i < len(path)-1; i++ {
		srcId := path[i]
		targetId := path[i+1]

		if adjMap, ok := adjMatrix[srcId]; ok {
			if edge, ok := adjMap[targetId]; ok {
				rel := edge.Properties.Data.(domain.Relation)
				relations = append(relations, rel)
				nodesOnPath = append(nodesOnPath, kg.Entities[targetId])
			}
		}
	}

	return Path{
		Nodes: nodesOnPath, Relations: relations}, nil
}

func (kg BuiltInKnowledgeBase) GetPath(source, target string) (Path, error) {
	path, err := graph.ShortestPath(kg.graph, source, target)
	if err != nil {
		return Path{}, err
	}

	return resolvePath(kg, path, source)
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
				ns = domain.NewNamespace(nsName)
				newEntities = append(newEntities, ns)
			}
			relations = append(relations, domain.Contains{Container: ns, Object: e.(domain.Entity)})
			return true
		}
		return false
	}

	mapPod := func(pod domain.Pod) {
		wl, rel := getWorkloadFromPod(pod)
		// mapNsRelation(pod)
		if wl != nil {
			newEntities = append(newEntities, wl)
			mapNsRelation(wl)
		} else {
			mapNsRelation(pod)
		}
		if rel != nil {
			relations = append(relations, rel)
		}
	}

	switch e := entity.(type) {
	case domain.Pod:
		mapPod(e)
	case domain.ApiServer: // sadly Go has no proper way to deal with type hierarchies, so a dedicated case is necessary ... sigh
		mapPod(e.Pod)
	default:
		if n, ok := entity.(domain.Namespaced); ok {
			mapNsRelation(n)
		}
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
	// if owner == nil {
	// 	id := uuid.New()

	// 	ownerName := pod.GetName()
	// 	if matches := WorkloadNamePattern.FindStringSubmatch(ownerName); len(matches) > 1 {
	// 		ownerName = matches[WorkloadNamePattern.SubexpIndex("workload")]
	// 	}

	// 	owner = domain.AbstractWorkload{
	// 		K8sEntity: domain.K8sEntity{
	// 			Id:        id.String(),
	// 			Name:      ownerName,
	// 			Kind:      "AbstractWorkload",
	// 			Namespace: pod.GetNamespace(),
	// 		},
	// 		ResourceOwner: resOwner,
	// 	}
	// }

	// TODO for a static pod the owner is actually the Node, think of way to properly model this

	var rel domain.Relation
	if owner != nil {
		rel = domain.Owns{
			Owner:  owner,
			Object: &pod,
		}
	}
	return owner, rel
}
