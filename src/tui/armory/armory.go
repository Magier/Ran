package armory

import (
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/tui/theme"
	"github.com/charmbracelet/bubbles/list"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type Action struct {
	title, desc string
}
type ActionSelected struct {
	Action domain.Message
}

func (a Action) Title() string       { return a.title }
func (a Action) Description() string { return a.desc }
func (a Action) FilterValue() string { return a.title }

type Model struct {
	actions list.Model
	focused bool
	style   lipgloss.Style
	width   float32
}

func NewAmory(width float32) Model {
	actions := []list.Item{
		Action{title: "Create Listener", desc: "Catch incoming shells"},
		Action{title: "Create Redirector", desc: "Create a proxy routing traffic to the C2"},
		Action{title: "Get Environment Variables", desc: "EnvVars can have secrets or interesting configurations"},
	}
	armoryList := list.New(actions, list.NewDefaultDelegate(), 40, 30)
	armoryList.Title = "Armory"
	armoryList.SetShowStatusBar(false)
	var style = lipgloss.NewStyle().
		Bold(true).
		// Foreground(lipgloss.Color("#7D56F4")).
		// Background(lipgloss.Color("#FAFAFA")).
		PaddingTop(1).
		PaddingLeft(1).
		Height(35).
		Width(40).
		BorderStyle(lipgloss.RoundedBorder()).
		BorderForeground(theme.InactiveColor).
		BorderTop(true).
		BorderLeft(true).
		BorderRight(true).
		BorderBottom(true)

	return Model{
		actions: armoryList,
		focused: false,
		style:   style,
		width:   width,
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	var cmds []tea.Cmd
	var cmd tea.Cmd

	if m.focused {
		m.actions, cmd = m.actions.Update(msg)
		cmds = append(cmds, cmd)
		switch msg := msg.(type) {
		case tea.KeyMsg:
			switch msg.Type {
			case tea.KeyEnter:
				cmd = func() tea.Msg {
					return ActionSelected{Action: domain.StartListener{Port: 1337}}
				}
				cmds = append(cmds, cmd)
			}
		case tea.WindowSizeMsg:
			m.style = m.style.Width(int(m.width * float32(msg.Width)))
			m.style = m.style.Height(msg.Height - 2) // -1 for the statusbar
		}
	}

	return m, tea.Batch(cmds...)
}

func (m Model) View() string {
	s := m.actions.View()

	if m.focused {
		activeStyle := m.style.BorderForeground(theme.PrimaryColor)
		return activeStyle.Render(s)
	} else {
		return m.style.Render(s)
	}
}

func (m *Model) Focus() {
	m.focused = true
	m.actions.SetShowHelp(true)
}
func (m *Model) Blur() {
	m.focused = false
	m.actions.SetShowHelp(true)
}
