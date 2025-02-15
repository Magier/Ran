package armory

import (
	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	tuimsg "github.com/Magier/Ran/tui/messages"
	"github.com/Magier/Ran/tui/theme"
	"github.com/charmbracelet/bubbles/list"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

const ellipsis = "…"

type Action struct {
	ID           string
	title, desc  string
	requirements domain.Requirements
	params       map[string]domain.Parameter
	args         map[string]string
}

type ActionSelected struct {
	ActionID string
	Action   domain.Message
	Args     map[string]any
}

func (a Action) Title() string       { return a.title }
func (a Action) Description() string { return a.desc }
func (a Action) FilterValue() string { return a.title }

type Model struct {
	actions list.Model
	focused bool
	style   lipgloss.Style
	width   float32
	target  tuimsg.EntitySelected
	state   domain.State
}

func NewArmory(armory armory.Armory, width float32) Model {
	actions := []list.Item{}

	for _, ttp := range armory.GetTTPs() {
		actions = append(actions, Action{
			ID:           ttp.GetID(),
			title:        ttp.GetTitle(),
			desc:         ttp.GetDescription(),
			requirements: ttp.Requires,
			params:       ttp.Params,
			args:         ttp.Args,
		})
	}

	armoryList := list.New(actions, NewActionItemDelegate(), 40, 30)
	// armoryList.Title = "Armory"
	armoryList.SetShowStatusBar(false)
	var style = lipgloss.NewStyle().
		Bold(true).
		PaddingLeft(1).
		BorderStyle(lipgloss.RoundedBorder()).
		BorderForeground(theme.InactiveColor).
		BorderTop(true).
		BorderLeft(true).
		BorderRight(true).
		BorderBottom(false)

	return Model{
		actions: armoryList,
		focused: false,
		style:   style,
		width:   width,
		state:   tuimsg.NewState(),
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	var cmds []tea.Cmd
	var cmd tea.Cmd

	switch msg := msg.(type) {
	case tea.KeyMsg:
		if m.focused {
			switch msg.Type {
			case tea.KeyEnter:
				selectedIdx := m.actions.Index()
				actions := m.actions.Items()
				action := actions[selectedIdx].(Action)
				cmd = func() tea.Msg { return ActionSelected{ActionID: action.Title()} }
				cmds = append(cmds, cmd)
			}
		}
	case tuimsg.EntitySelected:
		m.target = msg
	case c2.ListenerReady:
		m.state = m.state.Update("listener", 1)
		m.actions, cmd = m.actions.Update(tuimsg.StateChanged{State: m.state})
		cmds = append(cmds, cmd)
	case c2.ListenerStopped:
		m.state = m.state.Update("listener", -1)
		m.actions, cmd = m.actions.Update(tuimsg.StateChanged{State: m.state})
		cmds = append(cmds, cmd)
		// m.state = msg.State
	case tea.WindowSizeMsg:
		m.style = m.style.Width(int(m.width * float32(msg.Width)))
		h := msg.Height - 1 // -1 for the border
		m.style = m.style.Height(h)
		m.actions.SetHeight(h)
	}

	// only update key events, if it's actually targeting the ArmoryWindow
	if _, ok := msg.(tea.KeyMsg); !ok || m.focused {
		m.actions, cmd = m.actions.Update(msg)
		cmds = append(cmds, cmd)
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
	// m.actions.SetShowHelp(true)
}
func (m *Model) Blur() {
	m.focused = false
	m.actions.SetShowHelp(false)
}
