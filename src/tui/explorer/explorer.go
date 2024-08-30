package explorer

import (
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/tui/theme"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type Node struct {
	name       string
	kind       string
	children   []*Node
	isExpanded bool
	// isExpanded() bool
}

func (n Node) isLeaf() bool {
	return len(n.children) == 0
}

// Rework to ordered map
type Model struct {
	root    *Node
	focused bool
	width   float32
	style   lipgloss.Style
	cursor  int
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
		root:    root,
		focused: false,
		width:   width,
		style:   style,
		cursor:  -1,
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func newNode(name string, kind string) *Node {
	return &Node{
		name:       name,
		kind:       kind,
		isExpanded: false,
		children:   make([]*Node, 0),
	}
}

func (m *Model) numVisibleEntries() int {
	i := len(m.root.children)
	for _, node := range m.root.children {
		if node.isExpanded {
			i += len(node.children)
		}
	}
	return i
}

func addEntry(root *Node, groupPath []string, entry string, kind string) {
	currGroup := root
	if groupPath != nil {
		// walk to the correct node denoted by the groupPath
		for _, groupName := range groupPath {
			childIdx := -1
			for i, child := range currGroup.children {
				if child.name == groupName {
					childIdx = i
					break
				}
			}
			if childIdx == -1 {
				g := newNode(groupName, kind)
				// TODO: add proper sorting of entries
				currGroup.children = append(currGroup.children, g)
				childIdx = len(currGroup.children) - 1
			}
			currGroup = currGroup.children[childIdx]
		}
	}
	// TODO: add proper sorting of entries
	currGroup.children = append(currGroup.children, newNode(entry, kind))
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	switch msg := msg.(type) {
	case c2.SessionStarted:
		addEntry(m.root, nil, msg.Session.Hostname, "Implant")
	case domain.NewEntities:
		for _, pod := range msg.Pods {
			groupPath := []string{pod.Namespace}

			if pod.Labels != nil {
				name, ok := pod.Labels["app.kubernetes.io/name"]
				if ok {
					groupPath = append(groupPath, name) // add as workload name
				}
			}
			addEntry(m.root, groupPath, pod.Name, "Pod")
		}
	case tea.KeyMsg:
		switch msg.String() {
		// The "up" and "k" keys move the cursor up
		case "up", "k":
			if m.cursor > 0 {
				m.cursor--
			}

		// The "down" and "j" keys move the cursor down
		case "down", "j":
			if m.cursor < m.numVisibleEntries()-1 {
				m.cursor++
			}

		// The "enter" key and the spacebar (a literal space) toggle
		// the selected state for the item that the cursor is pointing at.
		case "enter", " ":
			// _, ok := m.Entries[m.cursor]
			// if ok {
			// 	// delete(m.selected, m.cursor)
			// } else {
			// 	// m.selected[m.cursor] = struct{}{}
			// }
		}
	case tea.WindowSizeMsg:
		h := msg.Height - 10 // -1 for the statusbar and top border
		m.style = m.style.Width(int(m.width * float32(msg.Width)))
		m.style = m.style.Height(h).MaxHeight(h)
	}
	return m, nil
}

func renderTree(root *Node, level int) []string {
	lines := make([]string, 0)
	// builder := strings.Builder{}
	indent := strings.Repeat("  ", level*2)
	for _, child := range root.children {
		lines = append(lines, indent+child.name)
		if child.isExpanded {
			lines = append(lines, renderTree(child, level+1)...)
		}
	}
	return lines
}

func (m Model) View() string {
	selectedStyle := lipgloss.NewStyle().Bold(true).Foreground(theme.PrimaryColor)

	var s string

	lines := renderTree(m.root, 0)
	// lines := renderLines(&m.Entries, 0)
	if m.cursor >= 0 {
		lines[m.cursor] = selectedStyle.Render(lines[m.cursor])
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
