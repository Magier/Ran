package explorer

import (
	"fmt"
	"log"
	"log/slog"
	"sort"

	"github.com/Magier/Ran/campaign"
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
	name   string
	kind   string
	id     string
	isPwnd bool
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

//	func (n *Node) At(index int) *Node {
//		return n.children[index]
//	}
//
//	func (n *Node) Length() int {
//		return len(n.children)
//	}
func newTree() *Node {
	return &Node{
		isExpanded: true,
		children: Children{
			children: make([]*Node, 0),
		},
	}
}

func newNode(id, name, kind string) *Node {
	return &Node{
		name:       name,
		id:         id,
		kind:       kind,
		isPwnd:     false,
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

// Traverses the tree of children in postorder.
// Call the provided yiel dfunction for ever visited node.
// The return type of the yield function dictates, if the subtree of the node will be traversed as well.
func (n *Node) traverse(yield func(n *Node) bool) {
	sort.Slice(n.children.children, func(i, j int) bool {
		return n.children.children[i].name < n.children.children[j].name
	})

	for _, child := range n.children.children {
		if yield(child) {
			child.traverse(yield)
		}
	}
}

func addEntity(tree *Node, campaign *campaign.Campaign, entity domain.Entity, expandedNodes map[string]struct{}) {
	parentNodes := make([]*Node, 0)

	if expandedNodes == nil {
		expandedNodes = make(map[string]struct{})
	}

	if _, ok := entity.(domain.Namespace); ok {
		n := addNode(tree, parentNodes, entity.GetId(), entity.GetName(), entity.GetKind())
		if n != nil {
			_, isExpanded := expandedNodes[n.id]
			n.isExpanded = isExpanded
		}
		return
	}

	k8sEntity, ok := entity.(domain.Ownable)
	if !ok {
		slog.Debug(entity.GetName() + " " + entity.GetKind() + " Not a K8sEntity")
		return
	}

	namespaced, ok := entity.(domain.Namespaced)
	var nsName string
	if ok {
		nsName = namespaced.GetNamespace()
		if nsName == "" {
			nsName = "?"
		}
		id := "ns/" + nsName
		n := newNode(id, nsName, "Namespace")
		_, isExpanded := expandedNodes[id]
		n.isExpanded = isExpanded
		parentNodes = append(parentNodes, n)
	}

	ownerRef, ok := k8sEntity.GetOwner()
	if ok {
		ownerKind := ownerRef.Kind
		if ownerKind == "" {
			ownerKind = "Workload"
		}
		ownerId := domain.GenerateId(ownerRef.Name, ownerKind, nsName)
		ownerName := ownerRef.Name

		// find top level owner of the entity for grouping within a namespaces
		owner, ok := campaign.GetEntityById(ownerId)
		if !ok {
			e := entity.(domain.Namespaced)
			owner, ok = campaign.GetEntityByName(ownerName, e.GetNamespace())
		}
		// use information of the resolved owner, if possible
		if ok {
			ownerId = owner.GetId()
			ownerName = owner.GetName()
			if k := owner.GetKind(); k != "" {
				ownerKind = k
			}
		}
		_, isExpanded := expandedNodes[ownerId]
		n := newNode(ownerId, ownerName, ownerKind)
		n.isExpanded = isExpanded
		parentNodes = append(parentNodes, n)
	}

	n := addNode(tree, parentNodes, entity.GetId(), entity.GetName(), entity.GetKind())
	if n != nil {
		_, isExpanded := expandedNodes[n.id]
		n.isExpanded = isExpanded
		if pod, ok := entity.(domain.K8sEntity); ok {
			n.isPwnd = pod.AccessLevel > domain.NoAccess
		} else if sys, ok := entity.(domain.System); ok {
			n.isPwnd = sys.AccessLevel > domain.NoAccess
		}
	}
}

func findOrCreateParents(root *Node, parentNodes []*Node) (*Node, error) {
	currGroup := root
	if parentNodes != nil {
		// walk to the correct node denoted by the groupPath
		for _, parent := range parentNodes {
			childIdx := -1
			for i, child := range currGroup.children.children {
				if child.id == parent.id {
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
func addNode(root *Node, parentNodes []*Node, id, entry, kind string) *Node {
	// TODO: add proper sorting of entries
	parent, err := findOrCreateParents(root, parentNodes)
	if err != nil {
		log.Fatalf("Failed to find or create parent node: %v", err)
	}
	var node *Node
	if !hasChild(parent, id) {
		node = newNode(id, entry, kind)
		parent.children.children = append(parent.children.children, node)
	}
	return node
}

func hasChild(parent *Node, childId string) bool {
	for _, c := range parent.children.children {
		if c.id == childId {
			return true
		}
	}
	return false
}
