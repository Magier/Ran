package explorer

import (
	"fmt"
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/tui/icon"
	tuimsg "github.com/Magier/Ran/tui/messages"
	"github.com/Magier/Ran/tui/theme"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type Node struct {
	name       string
	kind       string
	id         string
	children   []*Node
	isExpanded bool
	// isExpanded() bool
}

type entry struct {
	text string
	ref  *Node
}

func (n Node) isLeaf() bool {
	return len(n.children) == 0
}

// Rework to ordered map
type Model struct {
	entitiesTree *Node
	entries      []entry
	focused      bool
	width        float32
	style        lipgloss.Style
	cursor       int
}

func NewExplorer(width float32) Model {
	root := &Node{
		isExpanded: true,
		children:   make([]*Node, 0),
	}

	var style = lipgloss.NewStyle().
		Bold(true).
		// Foreground(lipgloss.Color("#7D56F4")).
		// Background(lipgloss.Color("#FAFAFA")).
		PaddingTop(1).
		PaddingLeft(1).
		Height(35).
		Width(22).
		BorderStyle(lipgloss.RoundedBorder()).
		BorderForeground(theme.InactiveColor).
		BorderTop(true).
		BorderLeft(true).
		BorderRight(true).
		BorderBottom(true)

	return Model{
		entitiesTree: root,
		focused:      false,
		width:        width,
		style:        style,
		cursor:       -1,
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func newNode(id string, name string, kind string) *Node {
	return &Node{
		name:       name,
		id:         id,
		kind:       kind,
		isExpanded: false,
		children:   make([]*Node, 0),
	}
}

func (m *Model) numVisibleEntries() int {
	i := len(m.entitiesTree.children)
	for _, node := range m.entitiesTree.children {
		if node.isExpanded {
			i += len(node.children)
		}
	}
	return i
}

// Add an entry to the tree of entities
// returns true if the added entry is visible in TUI or not
func addEntry(root *Node, parentNodes []*Node, id string, entry string, kind string) {
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
	// TODO: add proper sorting of entries
	currGroup.children = append(currGroup.children, newNode(id, entry, kind))
}

func getIcon(kind string) string {
	switch kind {
	case "Namespace":
		return icon.Namespace
	case "Pod":
		return icon.Pod
	case "Container":
		return icon.Container
	case "Workload":
		return icon.Workload
	}
	return ""
}

func buildShownEntries(tree *Node, level int) []entry {
	lines := make([]entry, 0)
	indent := strings.Repeat("  ", level*2)
	for _, child := range tree.children {
		icon := getIcon(child.kind)
		text := indent + icon + " " + child.name
		if !child.isLeaf() {
			text += fmt.Sprintf(" (%d)", len(child.children))
		}
		lines = append(lines, entry{text: text, ref: child})
		if child.isExpanded {
			lines = append(lines, buildShownEntries(child, level+1)...)
		}
	}
	return lines
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	var cmd tea.Cmd = nil

	switch msg := msg.(type) {
	case c2.SessionStarted:
		addEntry(m.entitiesTree, nil, msg.Session.Id, msg.Session.Hostname, "Implant")
		m.entries = buildShownEntries(m.entitiesTree, 0)
		if m.cursor == -1 {
			m.cursor = 0
		}
	case domain.NewEntities:
		for _, pod := range msg.Pods {
			parentNodes := []*Node{newNode("", pod.GetNamespace(), "Namespace")}

			name, ok := pod.GetLabel("app.kubernetes.io/name")
			if ok {
				parentNodes = append(parentNodes, newNode("", name, "Workload")) // add as workload name
			}
			addEntry(m.entitiesTree, parentNodes, pod.GetId(), pod.GetPodName(), "Pod")
		}
		m.entries = buildShownEntries(m.entitiesTree, 0)
		if m.cursor == -1 {
			m.cursor = 0
		}
	case tea.KeyMsg:
		if m.focused {
			switch msg.String() {
			case "up", "k":
				if m.cursor > 0 {
					m.cursor--
				}

			case "down", "j":
				if m.cursor < len(m.entries)-1 {
					m.cursor++
				}
			case " ":
				m.entries[m.cursor].ref.isExpanded = !m.entries[m.cursor].ref.isExpanded
				m.entries = buildShownEntries(m.entitiesTree, 0)
			case "right", "l":
				m.entries[m.cursor].ref.isExpanded = true
				m.entries = buildShownEntries(m.entitiesTree, 0)
			case "left", "h":
				m.entries[m.cursor].ref.isExpanded = false
				m.entries = buildShownEntries(m.entitiesTree, 0)
			case "enter":
				cmd = selectEntity(m.entries[m.cursor].ref)
			}
		}
	case tea.WindowSizeMsg:
		h := msg.Height - 10 // -1 for the statusbar and top border
		m.style = m.style.Width(int(m.width * float32(msg.Width)))
		m.style = m.style.Height(h).MaxHeight(h)
	}
	return m, cmd
}

func selectEntity(node *Node) tea.Cmd {
	return func() tea.Msg {
		return tuimsg.EntitySelected{Id: node.id, Kind: node.kind, Name: node.name}
	}
}

func (m Model) View() string {
	var s string
	lineStyle := lipgloss.NewStyle().Foreground(theme.InactiveColor).Faint(true).Inline(true)

	lines := []string{}
	for _, entry := range m.entries {
		lines = append(lines, lineStyle.Render(entry.text))
	}

	selectedStyle := lipgloss.NewStyle().Bold(true).UnsetForeground().Foreground(theme.PrimaryColor)
	if m.cursor >= 0 {
		// use the raw text again to avoid conflicting styles
		lines[m.cursor] = selectedStyle.Render(m.entries[m.cursor].text)
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
