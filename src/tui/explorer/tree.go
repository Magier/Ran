package explorer

import (
	"fmt"
	"log"

	"github.com/Magier/Ran/domain"
)

type Node struct {
	name       string
	kind       string
	id         string
	children   []*Node
	isExpanded bool
	// isExpanded() bool
}

func (n Node) isLeaf() bool {
	return len(n.children) == 0
}

func (n Node) findChild(name, parent string, kind, parentKind string) (*Node, bool) {
	for _, child := range n.children {
		if child.name == name {
			return child, true
		}
	}
	return nil, false
}

func addEntity(m Model, entity domain.Entity) {
	parentNodes := make([]*Node, 0)

	k8sEntity, ok := entity.(domain.Ownable)
	if !ok {
		fmt.Println(entity.GetName() + " " + entity.GetKind() + " Not a K8sEntity")
		return
	}

	namespaced, ok := entity.(domain.Namespaced)
	if ok {
		parentNodes = append(parentNodes, newNode("", namespaced.GetNamespace(), "Namespace"))
	}

	ownerRef, ok := k8sEntity.GetOwner()
	if ok {
		ownerId := ownerRef.Uid
		ownerName := ownerRef.Name
		ownerKind := ownerRef.Kind

		// find top level owner of the entity for grouping within a namespaces
		owner, ok := m.campaign.GetEntityById(ownerRef.Uid)
		if !ok {
			e := entity.(domain.Namespaced)
			owner, ok = m.campaign.GetEntityByName(ownerRef.Name, e.GetNamespace())
		}
		// use information of the resolved owner, if possible
		if ok {
			ownerId = owner.GetId()
			ownerName = owner.GetName()
			ownerKind = owner.GetKind()
		}

		parentNodes = append(parentNodes, newNode(ownerId, ownerName, ownerKind))
	}
	addNode(m.entitiesTree, parentNodes, entity.GetId(), entity.GetName(), entity.GetKind())
}

func findorCreateParents(root *Node, parentNodes []*Node) (*Node, error) {
	currGroup := root
	if parentNodes != nil {
		// walk to the correct node denoted by the groupPath
		for _, parent := range parentNodes {
			childIdx := -1
			for i, child := range currGroup.children {
				if child.name == parent.name {
					childIdx = i
					break
				}
			}
			if childIdx == -1 {
				// g := newNode("", parent.name, "Namespace")
				// TODO: add proper sorting of entries
				currGroup.children = append(currGroup.children, parent)
				childIdx = len(currGroup.children) - 1
			}
			currGroup = currGroup.children[childIdx]
		}
	}
	return currGroup, nil
}

// Add an entry to the tree of entities
// returns true if the added entry is visible in TUI or not
func addNode(root *Node, parentNodes []*Node, id string, entry string, kind string) {
	// TODO: add proper sorting of entries
	parent, err := findorCreateParents(root, parentNodes)
	if err != nil {
		log.Fatalf("Failed to find or create parent node: %v", err)
	}
	parent.children = append(parent.children, newNode(id, entry, kind))
}
