package logwindow

import (
	"github.com/Magier/Ran/c2"
	tea "github.com/charmbracelet/bubbletea"
)

type Model struct {
	nummLines int
	lines     []string
}

func New(numLogLines int) Model {
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
	return s
}
