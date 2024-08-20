package logwindow

import (
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/tui/theme"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

var style = lipgloss.NewStyle().
	Bold(true).
	// Foreground(lipgloss.Color("#7D56F4")).
	// Background(lipgloss.Color("#FAFAFA")).
	PaddingTop(1).
	PaddingLeft(1).
	Height(6).
	Width(80).
	BorderStyle(lipgloss.RoundedBorder()).
	BorderForeground(theme.InactiveColor).
	BorderTop(true).
	BorderLeft(true).
	BorderRight(true).
	BorderBottom(true)

type Model struct {
	nummLines int
	lines     []string
}

func NewLogWindow(numLogLines int) Model {
	return Model{
		nummLines: numLogLines,
		lines:     make([]string, 0, numLogLines),
	}
}

func (m *Model) AddLine(line string) {
	m.lines = append(m.lines, line)
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	switch msg := msg.(type) {
	case c2.ListenerReady:
		m.lines = append(m.lines, msg.String())
	case c2.SessionStarted:
		m.lines = append(m.lines, msg.String())
	}
	return m, nil
}

func (m Model) View() string {
	var s string
	for _, res := range m.lines {
		s += res + "\n"
	}
	return style.Render(s)
}
