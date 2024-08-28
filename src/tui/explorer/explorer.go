package explorer

import (
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/tui/theme"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type ResourceGroup struct {
	name      string
	kind      string
	resources []string
	subGroups map[string]*ResourceGroup // TODO: needs to be reworked to sth. that respects order
	expanded  bool
}

// Rework to ordered map
type Model struct {
	Entries ResourceGroup
	focused bool
	width   float32
	style   lipgloss.Style
	cursor  int
}

func NewExplorer(width float32) Model {
	entries := ResourceGroup{
		resources: make([]string, 0),
		subGroups: make(map[string]*ResourceGroup, 0),
		expanded:  true,
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
		// Entries: make([]string, 0),
		Entries: entries,
		focused: false,
		width:   width,
		style:   style,
		cursor:  -1,
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func newResourceGroup(name string) ResourceGroup {
	return ResourceGroup{
		name:      name,
		kind:      "",
		resources: make([]string, 0),
		subGroups: make(map[string]*ResourceGroup, 0),
		expanded:  false,
	}
}

func (m *Model) numVisibleEntries() int {
	i := len(m.Entries.resources) + len(m.Entries.subGroups)

	for _, subGroup := range m.Entries.subGroups {
		i += len(subGroup.subGroups)
		if subGroup.expanded {
			i += len(subGroup.resources)
		}
	}
	return i
}

func addEntry(groups *ResourceGroup, groupPath []string, entry string) {
	currGroup := groups
	if groupPath != nil {
		// walk to the correct resource group denoted by the groupPath
		for _, groupName := range groupPath {
			if _, ok := currGroup.subGroups[groupName]; !ok {
				g := newResourceGroup(groupName)
				currGroup.subGroups[groupName] = &g
			}
			currGroup = currGroup.subGroups[groupName]
		}
		currGroup.resources = append(currGroup.resources, entry)
	} else { // global objects
		groups.resources = append(groups.resources, entry)
	}
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	switch msg := msg.(type) {
	case c2.SessionStarted:
		addEntry(&m.Entries, nil, msg.Session.Hostname)
		// m.Entries = append(m.Entries, msg.Session.Hostname)
	case domain.NewEntities:
		for _, pod := range msg.Pods {
			groupPath := []string{pod.Namespace}

			if pod.Labels != nil {
				name, ok := pod.Labels["app.kubernetes.io/name"]
				if ok {
					groupPath = append(groupPath, name) // add as workload name
				}
			}
			addEntry(&m.Entries, groupPath, pod.Name)
		}
		// m.Entries = append(m.Entries, msg.Pod.Name)
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

func renderLines(entries *ResourceGroup, level int) []string {
	lines := make([]string, 0)
	// builder := strings.Builder{}
	indent := strings.Repeat("  ", level*2)

	for _, e := range entries.resources {
		lines = append(lines, indent+e)
		// builder.WriteString(indent + e + "\n")
	}
	for _, subGroup := range entries.subGroups {
		lines = append(lines, indent+subGroup.name)
		// builder.WriteString(indent + subGroup.name + "\n")
		if subGroup.expanded {
			lines = append(lines, renderLines(subGroup, level+1)...)
			// builder.WriteString()
		}
	}

	// return builder.String()
	return lines
}

func (m Model) View() string {
	selectedStyle := lipgloss.NewStyle().Bold(true).Foreground(theme.PrimaryColor)

	var s string

	lines := renderLines(&m.Entries, 0)
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
