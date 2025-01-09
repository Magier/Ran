package tui

import (
	"strings"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

var (

	// General.
	// normal    = lipgloss.Color("#EEEEEE")
	subtle = lipgloss.AdaptiveColor{Light: "#D9DCCF", Dark: "#383838"}
	// highlight = lipgloss.AdaptiveColor{Light: "#874BFD", Dark: "#7D56F4"}
	// special   = lipgloss.AdaptiveColor{Light: "#43BF6D", Dark: "#73F59F"}
	// blends    = gamut.Blends(lipgloss.Color("#F25D94"), lipgloss.Color("#EDFF82"), 50)
	warn = lipgloss.AdaptiveColor{Light: "#F25D94", Dark: "#F25D94"}

	dialogBoxStyle = lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color("#874BFD")).
			Padding(1, 0).
			BorderTop(true).
			BorderLeft(true).
			BorderRight(true).
			BorderBottom(true)

	buttonStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#FFF7DB")).
			Background(lipgloss.Color("#888B7E")).
			Padding(0, 3).
			MarginRight(2).
			MarginTop(1)

	activeButtonStyle = buttonStyle.
				Foreground(lipgloss.Color("#FFF7DB")).
				Background(lipgloss.Color("#F25D94")).
				Underline(true)
)

type showModalMsg struct {
	// if set, only the prompt is shown, without rendering the rest of the UI
	hideRest bool
	text     string
	actions  []ModalAction
}
type closeModalMsg struct {
}

// func (m showModalMsg) String() string {
// 	return "Modal: " + m.text
// }

type ModalModel struct {
	Text         string
	Actions      []ModalAction
	HideRest     bool
	activeButton int
	// actionMap    map[string]tea.Msg
	screenWidth  int
	screenHeight int
	IsVisible    bool
}

type ModalAction struct {
	Label  string
	Action tea.Cmd
}

func NewModal() ModalModel {
	return ModalModel{
		HideRest:     true,
		activeButton: 0,
	}
}

func (m ModalModel) Init() tea.Cmd {
	return nil
}
func (m ModalModel) Update(msg tea.Msg) (ModalModel, tea.Cmd) {
	var cmd tea.Cmd
	switch msg := msg.(type) {
	case tea.KeyMsg:
		if m.IsVisible {
			switch msg.Type {
			case tea.KeyTab:
				m.activeButton = (m.activeButton + 1) % len(m.Actions)
			case tea.KeyShiftTab:
				numActions := len(m.Actions)
				// workaround because GO does not have math. sound modulo operation :(
				m.activeButton = ((m.activeButton-1)%numActions + numActions) % numActions
			case tea.KeyEnter:
				cmd = m.Actions[m.activeButton].Action
				m.IsVisible = false
			}
		}
	case tea.WindowSizeMsg:
		m.screenWidth = msg.Width
		m.screenHeight = msg.Height
	}
	return m, cmd
}

func (m *ModalModel) SetContent(text string, actions []ModalAction) {
	m.Text = text
	m.Actions = actions
}

func (m *ModalModel) Show() {
	m.IsVisible = true
}
func (m *ModalModel) Hide() {
	m.IsVisible = false
}

func (m ModalModel) View() string {
	doc := strings.Builder{}

	buttons := []string{}

	for i, action := range m.Actions {
		style := buttonStyle
		if m.activeButton == i {
			style = activeButtonStyle
		}
		buttons = append(buttons, style.Render(action.Label))
	}

	text := lipgloss.NewStyle().Width(50).Align(lipgloss.Center).Foreground(warn).Render(m.Text)

	ui := lipgloss.JoinVertical(
		lipgloss.Center, text,
		lipgloss.JoinHorizontal(lipgloss.Top, buttons...),
	)

	dialog := lipgloss.Place(m.screenWidth, m.screenHeight,
		lipgloss.Center, lipgloss.Center,
		dialogBoxStyle.Render(ui),
		// lipgloss.WithWhitespaceChars("猫咪"),
		lipgloss.WithWhitespaceForeground(subtle),
	)

	doc.WriteString(dialog + "\n\n")
	return doc.String()
}
