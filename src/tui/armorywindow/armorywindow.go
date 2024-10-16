package armory

import (
	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/tui/theme"
	"github.com/charmbracelet/bubbles/list"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type Action struct {
	title, desc string
	msg         domain.Message
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

func NewArmory(armory armory.Armory, width float32) Model {
	actions := []list.Item{}

	for _, ttp := range armory.GetTTPs() {
		actions = append(actions, Action{title: ttp.GetTitle(), desc: ttp.GetDescription(), msg: ttp.GetMessage()})
	}
	armoryList := list.New(actions, list.NewDefaultDelegate(), 40, 30)
	armoryList.Title = "Armory"
	armoryList.SetShowStatusBar(false)
	var style = lipgloss.NewStyle().
		Bold(true).
		// Foreground(lipgloss.Color("#7D56F4")).
		// Background(lipgloss.Color("#FAFAFA")).
		// PaddingTop(1).
		PaddingLeft(1).
		// Height(35).
		// Width(40).
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
				selectedIdx := m.actions.Cursor()
				cmd = func() tea.Msg {
					return ActionSelected{Action: m.actions.Items()[selectedIdx].(Action).msg}
				}
				cmds = append(cmds, cmd)
			}
		case tea.WindowSizeMsg:
			m.style = m.style.Width(int(m.width * float32(msg.Width)))
			h := msg.Height - 1 // -1 for the statusbar
			// slog.Info(fmt.Sprintf("Armory height: %d", h))
			m.style = m.style.Height(h)
			m.actions.SetHeight(h)
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
