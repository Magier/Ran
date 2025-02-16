package widgets

import (
	"github.com/charmbracelet/bubbles/textinput"
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
var (
	focusedStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("205"))
	blurredStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("240"))
	cursorStyle  = focusedStyle
	defaultStyle = lipgloss.NewStyle()
)

type ModalType int

const (
	// Show only the text with the action to acknowledge it
	Alert ModalType = iota
	// show content with option to confirm or cancel it
	Confirm
)

type ShowModalMsg struct {
	// if set, only the prompt is shown, without rendering the rest of the UI
	HideRest bool
	Title    string
	Text     string
	Fields   []FormField
	Actions  []ModalAction
	Type     ModalType
}
type CloseModalMsg struct {
}

type Element interface {
	View() string
}

type FormField struct {
	Label string
	Elem  Element
}

func (f FormField) Update(msg tea.Msg) (FormField, tea.Cmd) {
	switch e := f.Elem.(type) {
	case textinput.Model:
		newModel, cmd := e.Update(msg)
		f.Elem = newModel
		return f, cmd
	}
	return f, nil
}

func (f FormField) Value() string {
	switch e := f.Elem.(type) {
	case textinput.Model:
		return e.Value()
	}
	return ""
}

func (f *FormField) Blur() {
	switch e := f.Elem.(type) {
	case textinput.Model:
		e.Blur()
		f.Elem = e
	}
}
func (f *FormField) Focus() {
	switch e := f.Elem.(type) {
	case textinput.Model:
		e.Focus()
		f.Elem = e
	}
}

func (f FormField) View() string {
	var e string
	if f.Elem != nil {
		e = f.Elem.View()
	}
	return lipgloss.JoinHorizontal(lipgloss.Top, f.Label, e)
}

type ModalModel struct {
	Title        string
	Text         string
	Fields       []FormField
	Actions      []ModalAction
	HideRest     bool
	activeButton int
	focusedField int
	// actionMap    map[string]tea.Msg
	screenWidth  int
	screenHeight int
	IsVisible    bool
}

type ModalAction struct {
	Label  string
	Action func(map[string]string) tea.Cmd
}

func NewModal() ModalModel {
	return ModalModel{
		HideRest:     true,
		activeButton: 0,
	}
}

func (m ModalModel) Init() tea.Cmd {
	return textinput.Blink
}
func (m ModalModel) Update(msg tea.Msg) (ModalModel, tea.Cmd) {
	var cmds []tea.Cmd
	switch msg := msg.(type) {
	case tea.KeyMsg:
		if m.IsVisible {
			switch msg.Type {
			case tea.KeyTab, tea.KeyCtrlN:
				m.nextElement()
				m.activeButton = (m.activeButton + 1) % len(m.Actions)
			case tea.KeyShiftTab, tea.KeyCtrlP:
				m.prevElement()
				numActions := len(m.Actions)
				// workaround because GO does not have math. sound modulo operation :(
				m.activeButton = ((m.activeButton-1)%numActions + numActions) % numActions
			case tea.KeyEnter:
				fn := m.Actions[m.activeButton].Action
				m.IsVisible = false

				values := make(map[string]string)
				for _, field := range m.Fields {
					values[field.Label] = field.Value()
				}
				cmds = append(cmds, fn(values))
			case tea.KeyEscape:
				m.IsVisible = false
			}
		}
	case tea.WindowSizeMsg:
		m.screenWidth = msg.Width
		m.screenHeight = msg.Height
	}
	for i := range m.Fields {
		var cmd tea.Cmd
		m.Fields[i], cmd = m.Fields[i].Update(msg)
		cmds = append(cmds, cmd)
	}
	return m, tea.Batch(cmds...)
}

func (m *ModalModel) SetContent(text string, fields []FormField, actions []ModalAction) {
	m.Text = text
	m.Fields = fields
	m.Actions = actions
}

func (m *ModalModel) Show(hideRest bool) {
	m.IsVisible = true
	m.HideRest = hideRest
}
func (m *ModalModel) Hide() {
	m.IsVisible = false
}

func (m ModalModel) View() string {
	// doc := strings.Builder{}

	buttons := []string{}

	for i, action := range m.Actions {
		style := buttonStyle
		if m.activeButton == i {
			style = activeButtonStyle
		}
		buttons = append(buttons, style.Render(action.Label))
	}

	content := lipgloss.NewStyle().Width(100).Align(lipgloss.Center).Foreground(warn).Render(m.Text)

	for i, field := range m.Fields {
		style := defaultStyle
		if i == m.focusedField {
			style = focusedStyle
		}

		content += style.Render(field.View()) + "\n"
	}

	ui := lipgloss.JoinVertical(
		lipgloss.Center, content,
		lipgloss.JoinHorizontal(lipgloss.Top, buttons...),
	)

	// dialog := lipgloss.Place(m.screenWidth, m.screenHeight,
	// 	lipgloss.Center, lipgloss.Center,
	// 	dialogBoxStyle.Render(ui),
	// 	// lipgloss.WithWhitespaceChars("猫咪"),
	// 	lipgloss.WithWhitespaceForeground(subtle),
	// )

	// doc.WriteString(dialog + "\n\n")
	// doc.write
	// return doc.String()
	return dialogBoxStyle.Width(100).Render(ui)
}

// nextElement focuses the next input field
func (m *ModalModel) nextElement() {
	m.focusedField = (m.focusedField + 1) % len(m.Fields)

	// ensure right field has the focus
	for i := range m.Fields {
		m.Fields[i].Blur()
	}
	m.Fields[m.focusedField].Focus()
}

// prevElement focuses the previous input field
func (m *ModalModel) prevElement() {
	m.focusedField--
	// Wrap around
	if m.focusedField < 0 {
		m.focusedField = len(m.Fields) - 1
	}

	// ensure right field has the focus
	for i := range m.Fields {
		m.Fields[i].Blur()
	}
	m.Fields[m.focusedField].Focus()
}
