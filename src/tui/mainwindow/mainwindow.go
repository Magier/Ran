package mainwindow

import (
	tuimsg "github.com/Magier/Ran/tui/messages"
	"github.com/Magier/Ran/tui/theme"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type Model struct {
	focused      bool
	style        lipgloss.Style
	width        float32
	bottomOffset int
	content      string
}

func NewMainWindow(width float32, bottomOffset int) Model {
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
		Background(lipgloss.Color("#123123")).
		BorderRight(true)
	// BorderBottom(true)

	return Model{content: "", focused: false, style: style, width: width, bottomOffset: bottomOffset}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.style = m.style.Width(int(m.width * float32(msg.Width)))
		m.style = m.style.Height(msg.Height - m.bottomOffset - 1)
	case tuimsg.EntitySelected:
		// TODO resolve ID to the actual entity
		m.content = msg.Id + " " + msg.Kind + " " + msg.Name
	}

	return m, nil
}

func (m Model) View() string {
	s := m.content
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
