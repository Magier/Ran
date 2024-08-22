package explorer

import (
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/tui/theme"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type Model struct {
	Entries []string
	focused bool
	width   float32
	style   lipgloss.Style
}

func NewExplorer(width float32) Model {
	entries := []string{
		"localhost",
		"pod 1",
		"kube-api",
		"db",
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
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	switch msg := msg.(type) {
	case c2.SessionStarted:
		m.Entries = append(m.Entries, msg.Session.Hostname)
	case tea.WindowSizeMsg:
		m.style = m.style.Width(int(m.width * float32(msg.Width)))
		m.style = m.style.Height(msg.Height - 2) // -1 for the statusbar and top border
	}
	return m, nil
}

func (m Model) View() string {
	var s string

	for _, e := range m.Entries {
		s += " - " + e + "\n"
	}

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
