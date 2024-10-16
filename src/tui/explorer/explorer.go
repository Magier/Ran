package explorer

import (
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/tui/icon"
	tuimsg "github.com/Magier/Ran/tui/messages"
	"github.com/Magier/Ran/tui/theme"
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
}

func NewExplorer(c *campaign.Campaign, width float32) Model {
	root := &Node{
		isExpanded: true,
		children: Children{
			children: make([]*Node, 0),
		},
	}

	var style = lipgloss.NewStyle().
		Bold(true).
		// Foreground(lipgloss.Color("#7D56F4")).
		// Background(lipgloss.Color("#FAFAFA")).
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
		campaign:     c,
		entitiesTree: root,
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

func (m *Model) numVisibleEntries() int {
	i := m.entitiesTree.Children().Length()
	for _, node := range m.entitiesTree.children.children {
		if node.isExpanded {
			i += node.Children().Length()
		}
	}
	return i
}

func getIcon(kind string) string {
	switch kind {
	case "Namespace":
		return icon.Namespace
	case "Pod":
		return icon.Pod
	case "Container":
		return icon.Container
	case "Workload", "Deployment", "StatefulSet", "DaemonSet":
		return icon.Workload
	case "Job":
		return icon.Job
	case "CronJob":
		return icon.CronJob
	}
	return ""
}

func buildShownEntries(tree *Node, level int) []entry {
	lines := make([]entry, 0)
	indent := strings.Repeat("  ", level*2)
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

func buildShownTree(m Model, orig *Node) *tree.Tree {
	t := tree.New().Enumerator(MyEnumerator)
	var _ tree.Node = orig.children.children[0]

	for _, child := range orig.children.children {
		sub := tree.Root(child)
		if child.children.Length() > 0 {
			nodes := buildShownTree(m, child)
			sub.Child(nodes)
		}
		t.Child(sub)
	}
	return t
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	var cmd tea.Cmd = nil

	switch msg := msg.(type) {
	case c2.SessionStarted:
		addNode(m.entitiesTree, nil, msg.Session.Id, msg.Session.Hostname, "Implant")
		m.entries = buildShownEntries(m.entitiesTree, 0)
		// m.tree = buildShownTree(m, m.entitiesTree)
		if m.cursor == -1 {
			m.cursor = 0
		}
	case domain.NewEntities:
		for _, entity := range msg.Entities {
			addEntity(m, entity)
		}
		m.entries = buildShownEntries(m.entitiesTree, 0)
		// m.tree = buildShownTree(m, m.entitiesTree)
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
				// m.tree.Children().At(m.cursor)
			case "right", "l":
				m.entries[m.cursor].ref.isExpanded = true
				m.entries = buildShownEntries(m.entitiesTree, 0)
			case "left", "h":
				m.entries[m.cursor].ref.isExpanded = false
				m.entries = buildShownEntries(m.entitiesTree, 0)
			case "g": // go to the top
				m.cursor = 0
			case "G": // go to the bottom
				m.cursor = len(m.entries) - 1
			case "enter":
				cmd = selectEntity(m.entries[m.cursor].ref)
			}
		}
	case tea.WindowSizeMsg:
		h := msg.Height - 1 // -1 for the statusbar and top border
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
	// TODO: implement scrolling for big lists
	var s string
	lineStyle := lipgloss.NewStyle().Faint(true).Inline(true)
	// lineStyle := lipgloss.NewStyle().Foreground(theme.InactiveColor).Faint(true).Inline(true)

	// highlightSelectedFunc := func(_ tree.Children, i int) lipgloss.Style {
	// 	if m.cursor == i {
	// 		return lipgloss.NewStyle().Background(lipgloss.Color("#7D56F4"))
	// 	}
	// 	return lipgloss.NewStyle().Faint(true)
	// }
	// m.tree.ItemStyleFunc(highlightSelectedFunc)

	lines := []string{}
	for _, entry := range m.entries {
		lines = append(lines, lineStyle.Render(entry.text))
	}

	selectedStyle := lipgloss.NewStyle().Bold(true).UnsetForeground().Background(theme.PrimaryColor)
	if m.cursor >= 0 {
		// use the raw text again to avoid conflicting styles
		lines[m.cursor] = selectedStyle.Render(m.entries[m.cursor].text)
	}
	s += strings.Join(lines, "\n")
	// s += m.tree.String()

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
	return m.entries[m.cursor].ref.id
}
