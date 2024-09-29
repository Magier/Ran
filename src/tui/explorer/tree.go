package explorer

import (
	"fmt"
	"log"

	"github.com/Magier/Ran/domain"
	tree "github.com/charmbracelet/lipgloss/tree"
)

type Children struct {
	children []*Node
}

func (c Children) At(index int) tree.Node {
	return c.children[index]
}
func (c Children) Length() int {
	return len(c.children)
}

type Node struct {
	name string
	kind string
	id   string
	// children   []*Node
	isExpanded bool
	children   Children
}

func (n *Node) String() string {
	icon := getIcon(n.kind)
	s := fmt.Sprintf("%s %s", icon, n.name)
	if !n.isLeaf() {
		s += fmt.Sprintf(" (%d)", n.children.Length())
	}
	return s
}

func (n *Node) Value() string {
	return n.name
}

func (n *Node) Children() tree.Children {
	return n.children
}

func (n *Node) Hidden() bool {
	return !n.isExpanded
}

// func (n *Node) At(index int) *Node {
// 	return n.children[index]
// }
// func (n *Node) Length() int {
// 	return len(n.children)
// }

func newNode(id string, name string, kind string) *Node {
	return &Node{
		name:       name,
		id:         id,
		kind:       kind,
		isExpanded: false,
		children:   Children{children: make([]*Node, 0)},
		// children:   make([]*Node, 0),
	}
}

func (n Node) isLeaf() bool {
	return n.children.Length() == 0
}

func (n Node) findChild(name, parent string, kind, parentKind string) (*Node, bool) {
	for _, child := range n.children.children {
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

func findOrCreateParents(root *Node, parentNodes []*Node) (*Node, error) {
	currGroup := root
	if parentNodes != nil {
		// walk to the correct node denoted by the groupPath
		for _, parent := range parentNodes {
			childIdx := -1
			for i, child := range currGroup.children.children {
				if child.name == parent.name {
					childIdx = i
					break
				}
			}
			if childIdx == -1 {
				// g := newNode("", parent.name, "Namespace")
				// TODO: add proper sorting of entries
				currGroup.children.children = append(currGroup.children.children, parent)
				childIdx = len(currGroup.children.children) - 1
			}
			currGroup = currGroup.children.children[childIdx]
		}
	}
	return currGroup, nil
}

// Add an entry to the tree of entities
// returns true if the added entry is visible in TUI or not
func addNode(root *Node, parentNodes []*Node, id string, entry string, kind string) {
	// TODO: add proper sorting of entries
	parent, err := findOrCreateParents(root, parentNodes)
	if err != nil {
		log.Fatalf("Failed to find or create parent node: %v", err)
	}
	parent.children.children = append(parent.children.children, newNode(id, entry, kind))

}
