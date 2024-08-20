package mainwindow

import (
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
	Height(26).
	Width(80).
	BorderStyle(lipgloss.RoundedBorder()).
	BorderForeground(theme.InactiveColor).
	BorderTop(true).
	BorderLeft(true).
	BorderRight(true)
	// BorderBottom(true)

type Model struct {
	focused bool
}

func NewMainWindow() Model {
	return Model{focused: false}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	return m, nil
}

func (m Model) View() string {
	s := "MainWindow"
	if m.focused {
		activeStyle := style.BorderForeground(theme.PrimaryColor)
		return activeStyle.Render(s)
	} else {
		return style.Render(s)
	}
}

func (m *Model) Focus() {
	m.focused = true
}
func (m *Model) Blur() {
	m.focused = false
}
