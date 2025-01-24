package explorer

import (
	"sort"
	"strings"

	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/tui/icon"
	tuimsg "github.com/Magier/Ran/tui/messages"
	"github.com/Magier/Ran/tui/theme"
	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	tree "github.com/charmbracelet/lipgloss/tree"
)

type entry struct {
	text string
	ref  *Node
}

func (e entry) String() string {
	return e.text
}

// Rework to ordered map
type Model struct {
	campaign     *campaign.Campaign
	entitiesTree *Node
	tree         *tree.Tree
	entries      []entry
	focused      bool
	width        float32
	style        lipgloss.Style
	cursor       int
	viewport     viewport.Model
}

func NewExplorer(c *campaign.Campaign, width float32) Model {
	var style = lipgloss.NewStyle().
		Bold(true).
		// Foreground(lipgloss.Color("#7D56F4")).
		// Background(lipgloss.Color("#FAFAFA")).
		PaddingLeft(1).
		PaddingTop(1).
		// Height(35).
		Width(22).
		BorderStyle(lipgloss.RoundedBorder()).
		BorderForeground(theme.InactiveColor).
		BorderTop(true).
		BorderLeft(true).
		BorderRight(true).
		BorderBottom(false)

	return Model{
		campaign:     c,
		entitiesTree: newTree(),
		tree:         tree.New(),
		focused:      false,
		width:        width,
		style:        style,
		cursor:       -1,
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

// func (m *Model) numVisibleEntries() int {
// 	i := m.entitiesTree.Children().Length()
// 	for _, node := range m.entitiesTree.children.children {
// 		if node.isExpanded {
// 			i += node.Children().Length()
// 		}
// 	}
// 	return i
// }

func getIcon(kind string) string {
	switch kind {
	case "Namespace":
		return icon.Namespace
	case "Pod":
		return icon.Pod
	case "Container":
		return icon.Container
	case "AbstractWorkload", "Deployment", "StatefulSet", "DaemonSet":
		return icon.Workload
	case "Job":
		return icon.Job
	case "CronJob":
		return icon.CronJob
	case "Node":
		return icon.WorkerNode
	case "ServiceAccount":
		return icon.Identity
	}
	return ""
}

func buildShownEntries(tree *Node, level int) []entry {
	lines := make([]entry, 0)
	indent := strings.Repeat(" ", level*2)

	sort.Slice(tree.children.children, func(i, j int) bool {
		return tree.children.children[i].name < tree.children.children[j].name
	})

	for _, child := range tree.children.children {
		text := indent + child.String()
		lines = append(lines, entry{text: text, ref: child})
		if child.isExpanded {
			lines = append(lines, buildShownEntries(child, level+1)...)
		}
	}
	return lines
}

func MyEnumerator(children tree.Children, index int) string {
	if children.Length() == 0 {
		return ""
	}
	a := children.At(0)
	_ = a
	// c := .(*Node)
	// if c.kind == "Namespace" {
	// 	return ""
	// }
	if children.Length()-1 == index {
		return "╰──"
	}
	return "├──"
}

// func buildShownTree(m Model, orig *Node) *tree.Tree {
// 	t := tree.New().Enumerator(MyEnumerator)

// 	for _, child := range orig.children.children {
// 		sub := tree.Root(child)
// 		if child.children.Length() > 0 {
// 			nodes := buildShownTree(m, child)
// 			sub.Child(nodes)
// 		}
// 		t.Child(sub)
// 	}
// 	return t
// }

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	var cmd tea.Cmd = nil

	switch msg := msg.(type) {
	case domain.KnowledgeUpdated:
		m = m.rebuildEntries()
		if m.cursor == -1 && len(m.entries) > 0 {
			m.cursor = 0
			cmd = selectEntity(m.entries[m.cursor].ref)
		}
	case tea.KeyMsg:
		if m.focused {
			switch msg.String() {
			case "up", "k":
				if m.cursor > 0 {
					m.cursor--
				}
				cmd = selectEntity(m.entries[m.cursor].ref)
			case "down", "j":
				if m.cursor < len(m.entries)-1 {
					m.cursor++
				}
				cmd = selectEntity(m.entries[m.cursor].ref)
			case " ":
				m.entries[m.cursor].ref.isExpanded = !m.entries[m.cursor].ref.isExpanded
				m.entries = buildShownEntries(m.entitiesTree, 0)
			case "right", "l":
				m.entries[m.cursor].ref.isExpanded = true
				m.entries = buildShownEntries(m.entitiesTree, 0)
			case "left", "h":
				m.entries[m.cursor].ref.isExpanded = false
				m.entries = buildShownEntries(m.entitiesTree, 0)
			case "g": // go to the top
				m.cursor = 0
				cmd = selectEntity(m.entries[m.cursor].ref)
			case "G": // go to the bottom
				m.cursor = len(m.entries) - 1
				cmd = selectEntity(m.entries[m.cursor].ref)
			case "enter":
				// if m.cursor >= 0 {
				// 	cmd = selectEntity(m.entries[m.cursor].ref)
				// }
			}
		}
	case tea.WindowSizeMsg:
		h := msg.Height - 1 // -1 for the top border
		m.style = m.style.Width(int(m.width * float32(msg.Width)))
		m.style = m.style.Height(h) //.MaxHeight(h)
	}
	return m, cmd
}

func (m Model) rebuildEntries() Model {
	// keep track of previously opened nodes
	expandedNodes := make(map[string]struct{})
	m.entitiesTree.traverse(func(n *Node) bool {
		if n.isExpanded {
			expandedNodes[n.id] = struct{}{}
		}
		return n.isExpanded
	})

	// start new tree from scratch
	m.entitiesTree = newTree()
	entities := m.campaign.GetEntities()
	for _, entity := range entities {
		addEntity(m.entitiesTree, m.campaign, entity, expandedNodes)
	}
	m.entries = buildShownEntries(m.entitiesTree, 0)
	return m
}

func selectEntity(node *Node) tea.Cmd {
	return func() tea.Msg {
		return tuimsg.EntitySelected{Id: node.id, Kind: node.kind, Name: node.name, AccessLevel: node.accessLevel}
	}
}

func (m Model) View() string {
	// TODO: implement scrolling for big lists
	var s string
	lineStyle := lipgloss.NewStyle().Faint(true).Inline(true)
	pwndStyle := lipgloss.NewStyle().Faint(true).Inline(true).Foreground(theme.PositiveColor)

	lines := []string{}
	var style lipgloss.Style
	for _, entry := range m.entries {
		style = lineStyle
		if entry.ref.isPwnd {
			style = pwndStyle
		}

		lines = append(lines, style.Render(entry.text))
	}

	selectedStyle := lipgloss.NewStyle().Bold(true).UnsetForeground().Background(theme.PrimaryColor)
	if m.cursor >= 0 && len(lines) > 0 {
		// use the raw text again to avoid conflicting styles
		e := m.entries[m.cursor]
		lines[m.cursor] = selectedStyle.Render(e.text)
	}
	s += strings.Join(lines, "\n")

	if m.focused {
		activeStyle := m.style.BorderForeground(theme.PrimaryColor)
		return activeStyle.Render(s)
	} else {
		return m.style.Render(s)
	}
}

func (m *Model) Focus() {
	m.focused = true
}
func (m *Model) Blur() {
	m.focused = false
}

func (m Model) GetSelectedEntity() string {
	if m.cursor < 0 {
		return ""
	}
	return m.entries[m.cursor].ref.id
}
