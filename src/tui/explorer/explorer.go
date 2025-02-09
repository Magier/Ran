package explorer

import (
	"fmt"
	"strings"

	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/tui/icon"
	tuimsg "github.com/Magier/Ran/tui/messages"
	"github.com/Magier/Ran/tui/theme"
	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type entry struct {
	level       int
	label       string
	entity      domain.Entity
	isExpanded  bool
	isPwnd      bool
	accessLevel domain.AccessLevel
	numChildren int
}

func (e entry) Label() string {
	indent := strings.Repeat(" ", e.level*2)
	icon := getIcon(e.entity.GetKind())
	label := fmt.Sprintf("%s %s", icon, e.entity.GetName())
	if !e.isExpanded && e.numChildren > 0 {
		label += fmt.Sprintf(" (%d)", e.numChildren)
	}
	return indent + label
}

// Rework to ordered map
type Model struct {
	campaign *campaign.Campaign
	entries  []entry
	entities []entry
	focused  bool
	width    float32
	style    lipgloss.Style
	cursor   int
	viewport viewport.Model
}

func NewExplorer(c *campaign.Campaign, width float32) Model {
	var style = lipgloss.NewStyle().
		Bold(true).
		// Foreground(lipgloss.Color("#7D56F4")).
		// Background(lipgloss.Color("#FAFAFA")).
		PaddingLeft(1).
		PaddingTop(0).
		// Height(35).
		Width(22).
		BorderStyle(lipgloss.RoundedBorder()).
		BorderForeground(theme.InactiveColor).
		BorderTop(true).
		BorderLeft(true).
		BorderRight(true).
		BorderBottom(false)

	return Model{
		campaign: c,
		focused:  false,
		width:    width,
		style:    style,
		cursor:   -1,
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func getIcon(kind string) string {
	switch kind {
	case "Namespace":
		return icon.Namespace
	case "Cluster":
		return icon.K8s
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
	case "Session":
		return icon.Session
	}
	return ""
}

// 	if children.Length()-1 == index {
// 		return "╰──"
// 	return "├──"
// }

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	var cmd tea.Cmd = nil

	switch msg := msg.(type) {
	case domain.KnowledgeUpdated:
		m = m.rebuildEntries()
		if m.cursor == -1 && len(m.entries) > 0 {
			m.cursor = 0
			cmd = selectEntity(m.entries[m.cursor])
		}
	case tea.KeyMsg:
		if m.focused {
			switch msg.String() {
			case "up", "k":
				if m.cursor > 0 {
					m.cursor--
				}
				cmd = selectEntity(m.entries[m.cursor])
			case "down", "j":
				if m.cursor < len(m.entries)-1 {
					m.cursor++
				}
				cmd = selectEntity(m.entries[m.cursor])
			case " ":
				e := m.entries[m.cursor]
				m.expandEntry(e, !e.isExpanded)
				// idx := getEntriesIndex(m.entities, m.entries[m.cursor].entity.GetId())
				// m.entities[m.cursor].isExpanded = !m.entries[m.cursor].isExpanded
				// m.entries = buildShownEntries(m.entities)
			case "right", "l":
				m.expandEntry(m.entities[m.cursor], true)
				// m.entries[m.cursor].isExpanded = true
				// m.entries = buildShownEntries(m.entities)
			case "left", "h":
				m.expandEntry(m.entities[m.cursor], false)
				// m.entries[m.cursor].isExpanded = false
				// m.entries = buildShownEntries(m.entities)
			case "g": // go to the top
				m.cursor = 0
				cmd = selectEntity(m.entries[m.cursor])
			case "G": // go to the bottom
				m.cursor = len(m.entries) - 1
				cmd = selectEntity(m.entries[m.cursor])
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

func (m *Model) expandEntry(entry entry, isExpanded bool) {
	idx := getEntriesIndex(m.entities, entry)
	if idx >= 0 {
		m.entities[idx].isExpanded = isExpanded
		m.entries = buildShownEntries(m.entities)
	}
}

func getEntriesIndex(entries []entry, entry entry) int {
	entryID := entry.entity.GetId()
	for i, e := range entries {
		if e.entity.GetId() == entryID {
			return i
		}
	}
	return -1
}

func (m Model) rebuildEntries() Model {
	// keep track of previously opened nodes
	expandedNodes := make(map[string]struct{})
	for _, e := range m.entries {
		expandedNodes[e.entity.GetId()] = struct{}{}
	}
	var checkExpandedFn func(entry) bool
	if len(m.entries) == 0 {
		checkExpandedFn = func(e entry) bool {
			return e.level <= 1
		}
	} else {
		checkExpandedFn = func(e entry) bool {
			_, isExpanded := expandedNodes[e.entity.GetId()]
			return isExpanded
		}
	}

	adj := m.campaign.GetGraph()
	entities := m.campaign.GetEntities()

	m.entities = make([]entry, 0)
	depthFirstTraversal(adj, "cluster", func(n string, level int) {
		if e, ok := entities[n]; ok {
			accessLevel, isPwnd := getAccessLevel(e)
			entry := entry{
				level:       level,
				entity:      e,
				numChildren: len(adj[e.GetId()]),
				accessLevel: accessLevel,
				isPwnd:      isPwnd,
			}
			entry.isExpanded = checkExpandedFn(entry)
			m.entities = append(m.entities, entry)
		}
	})

	m.entries = buildShownEntries(m.entities)
	return m
}

func getAccessLevel(entity domain.Entity) (domain.AccessLevel, bool) {
	var isPwnd bool
	var accessLevel domain.AccessLevel
	switch e := entity.(type) {
	case domain.Pod:
		isPwnd = e.AccessLevel != domain.NoAccess
		accessLevel = e.AccessLevel
	case domain.System:
		isPwnd = e.AccessLevel != domain.NoAccess
		accessLevel = e.AccessLevel
	}
	return accessLevel, isPwnd
}

func depthFirstTraversal(adjList map[string]map[string]string, startNode string, visit func(node string, level int)) {
	visited := make(map[string]bool)
	var dfs func(node string, level int)
	dfs = func(node string, level int) {
		if visited[node] {
			return
		}
		visited[node] = true
		visit(node, level)
		for neighbor := range adjList[node] {
			dfs(neighbor, level+1)
		}
	}
	dfs(startNode, 0)
}

func buildShownEntries(entries []entry) []entry {
	lines := make([]entry, 0)

	for i := 0; i < len(entries); i++ {
		e := entries[i]
		e.label = e.Label()
		lines = append(lines, e)
		if !e.isExpanded && e.numChildren > 0 {
			// skip all children
			skipLevel := e.level
			for i+1 < len(entries) && entries[i+1].level > skipLevel {
				i++
			}
		}
	}
	return lines
}

func selectEntity(e entry) tea.Cmd {
	return func() tea.Msg {
		return tuimsg.EntitySelected{Id: e.entity.GetId(), Kind: e.entity.GetKind(), Name: e.entity.GetName(), AccessLevel: e.accessLevel}
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
		if entry.isPwnd {
			style = pwndStyle
		}

		lines = append(lines, style.Render(entry.label))
	}

	selectedStyle := lipgloss.NewStyle().Bold(true).UnsetForeground().Background(theme.PrimaryColor)
	if m.cursor >= 0 && len(lines) > 0 {
		// use the raw text again to avoid conflicting styles
		e := m.entries[m.cursor]
		lines[m.cursor] = selectedStyle.Render(e.label)
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
	return m.entries[m.cursor].entity.GetId()
}
