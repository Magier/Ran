package explorer

import (
	"github.com/Magier/Ran/c2"
	tea "github.com/charmbracelet/bubbletea"
)

type Model struct {
	entries []string
}

func NewModel() Model {
	return Model{
		entries: make([]string, 0),
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	switch msg := msg.(type) {
	case c2.SessionStarted:
		m.entries = append(m.entries, msg.Session.Hostname)
	}
	return m, nil
}

func (m Model) View() string {
	var s string

	for _, e := range m.entries {
		s += " - " + e + "\n"
	}

	return s
}
