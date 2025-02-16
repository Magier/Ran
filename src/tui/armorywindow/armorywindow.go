package armory

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	tuimsg "github.com/Magier/Ran/tui/messages"
	"github.com/Magier/Ran/tui/theme"
	"github.com/Magier/Ran/tui/widgets"
	"github.com/charmbracelet/bubbles/list"
	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

const ellipsis = "…"

type Action struct {
	ID           string
	Name, Desc   string
	Requirements domain.Requirements
	Params       map[string]domain.Parameter
	Args         map[string]string
}

func (a Action) Title() string       { return a.Name }
func (a Action) Description() string { return a.Desc }
func (a Action) FilterValue() string { return a.Name }

func (a Action) GetFormFields() []widgets.FormField {

	fields := []widgets.FormField{}
	for name, param := range a.Params {
		var elem widgets.Element

		switch strings.ToLower(param.Type) {
		case "string":
			input := textinput.New()
			input.Placeholder = name
			input.SetValue(param.Default)
			input.Width = 100
			elem = input
		case "int":
			input := textinput.New()
			input.Placeholder = name
			input.SetValue(param.Default)
			input.Validate = intValidator
			elem = input
			// case "bool":
			// 	input :=
			// 	input.Placeholder = name
			// 	input.SetValue(param.Default)
			// 	input.Validate = intValidator
			// 	elem = input
		}
		fields = append(fields, widgets.FormField{
			Label: cases.Title(language.English, cases.NoLower).String(name),
			Elem:  elem,
		})
	}
	return fields
}
func intValidator(s string) error {
	// TODO check the range, if one is specified
	_, err := strconv.ParseInt(s, 10, 64)
	if err != nil {
		return fmt.Errorf("%s is not a valid number", s)
	}
	return nil
}

type ActionSelected struct {
	ActionID  string
	Action    Action
	ActionMsg domain.Message
	Args      map[string]any
}

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
			Name:         ttp.GetTitle(),
			Desc:         ttp.GetDescription(),
			Requirements: ttp.Requires,
			Params:       ttp.Params,
			Args:         ttp.Args,
		})
	}

	armoryList := list.New(actions, NewActionItemDelegate(), 40, 30)
	// armoryList.Title = "Armory"
	armoryList.SetShowTitle(false)
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
				action := m.actions.SelectedItem().(Action)
				m.actions.ResetFilter()
				cmd = func() tea.Msg { return ActionSelected{Action: action} }
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
