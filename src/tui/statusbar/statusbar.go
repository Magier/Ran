package statusbar

import (
	"fmt"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/tui/icon"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type ColorConfig struct {
	Foreground lipgloss.AdaptiveColor
	Background lipgloss.AdaptiveColor
}

var (
	primaryColor    = lipgloss.AdaptiveColor{Light: "#874BFD", Dark: "#7D56F4"}
	positiveColor   = lipgloss.AdaptiveColor{Light: "#43BF6D", Dark: "#73F59F"}
	negativeColor   = lipgloss.AdaptiveColor{Light: "#FF5F87", Dark: "#FF5B23"}
	backgroundColor = lipgloss.AdaptiveColor{Light: "#D9DCCF", Dark: "#353533"}
	passiveColor    = lipgloss.AdaptiveColor{Light: "#343433", Dark: "#C1C6B2"}

	defaultStyle = lipgloss.NewStyle().
			Height(1).
			Foreground(passiveColor).
			Background(backgroundColor)

	activeColorConfig = ColorConfig{
		Foreground: positiveColor,
		Background: backgroundColor,
	}

	neutralColorConfig = ColorConfig{
		Foreground: passiveColor,
		Background: backgroundColor,
	}
	negativeColorConfig = ColorConfig{
		Foreground: negativeColor,
		Background: backgroundColor,
	}
	// listenerStyle = statusNugget.Background(lipgloss.Color("#6124DF"))
)

// Height represents the height of the statusbar.
const Height = 1

type Field struct {
	icon  string
	title string
	color ColorConfig
}

// Model represents the properties of the statusbar.
type Model struct {
	c                   *campaign.Campaign
	Width               int
	c2ServerStatus      Field
	listenerStatus      Field
	identityStatus      Field
	selectedC2          string
	availableIdentities int
	listeners           map[string]c2.ListenerReady
}

func NewStatusBar(c *campaign.Campaign) Model {
	listeners := make(map[string]c2.ListenerReady)
	return Model{
		c:                   c,
		availableIdentities: 0,
		identityStatus: Field{
			title: "no identities",
			icon:  icon.Fingerprint,
		},
		c2ServerStatus: Field{
			title: "pending ...",
			icon:  icon.LanPending,
		},
		listeners:      listeners,
		listenerStatus: updateListenerStatus(listeners),
	}
}

// Update updates the size of the statusbar.
func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	var cmd tea.Cmd
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.Width = msg.Width
	case c2.C2ConnectFailed:
		m.selectedC2 = msg.Name
		m.c2ServerStatus.title = "not connected to " + msg.Name
		m.c2ServerStatus.icon = icon.LanDisconnect
		m.c2ServerStatus.color = negativeColorConfig
	case domain.ConnectedToExternalC2Server:
		var ipDetail string
		if msg.Ip != "0.0.0.0" && msg.Ip != "localhost" {
			ipDetail = fmt.Sprintf(" (%s)", msg.Ip)
		}
		m.selectedC2 = msg.Name
		m.c2ServerStatus.title = fmt.Sprintf("C2: %s%s", msg.Name, ipDetail)
		m.c2ServerStatus.icon = icon.LanConnect
		m.c2ServerStatus.color = activeColorConfig
	case c2.ListenerReady:
		m.listeners[msg.Name] = msg
		m.listenerStatus = updateListenerStatus(m.listeners)
	case c2.ListenerStopped:
		delete(m.listeners, msg.Name)
		m.listenerStatus = updateListenerStatus(m.listeners)
	case domain.KnowledgeUpdated:
		identities := m.c.GetIdentities()
		m.availableIdentities = len(identities)
		activeIdentity, ok := m.c.GetActiveIdentity()
		if ok {
			m.identityStatus.title = activeIdentity.Name
			m.identityStatus.color = activeColorConfig
		}
	case tea.MouseMsg:
		if msg.Action == tea.MouseActionPress && msg.Button == tea.MouseButtonLeft {
			c2StatusCol := renderField(m.c2ServerStatus)
			if msg.X < lipgloss.Width(c2StatusCol) {
				cmd = func() tea.Msg { return domain.StartC2{C2Name: m.selectedC2} }

				m.c2ServerStatus.icon = icon.LanPending
				m.c2ServerStatus.title = "connecting to " + m.selectedC2
				m.c2ServerStatus.color = neutralColorConfig
			}
		}
	}
	return m, cmd
}

func updateListenerStatus(listeners map[string]c2.ListenerReady) Field {
	numListeners := len(listeners)

	title := "no listeners"
	i := icon.Listen
	color := activeColorConfig

	if numListeners == 0 {
		i = "🔇"
		color = negativeColorConfig
	} else if numListeners == 1 {
		var listener c2.ListenerReady
		for _, l := range listeners {
			listener = l
			break
		}
		title = fmt.Sprintf("%s:%d", listener.Name, listener.Port)
	} else {
		title = fmt.Sprintf("%d listeners", numListeners)
	}

	return Field{
		title: title,
		icon:  i,
		color: color,
	}
}

func renderField(field Field) string {
	return lipgloss.NewStyle().
		Inherit(defaultStyle).
		Padding(0, 1).
		Foreground(field.color.Foreground).
		Background(field.color.Background).
		Render(field.icon + " " + field.title)
}

func (m Model) View() string {
	cols := []string{}
	remainingWidth := m.Width

	// left columns
	for _, field := range []Field{m.c2ServerStatus, m.identityStatus} {
		col := renderField(field)
		cols = append(cols, col)
		remainingWidth -= lipgloss.Width(col)
	}

	//right columns
	rightCols := []string{}
	for _, field := range []Field{m.listenerStatus} {
		col := defaultStyle.
			Foreground(field.color.Foreground).
			Background(field.color.Background).
			Padding(0, 1).
			Inherit(defaultStyle).
			Render(field.icon + " " + field.title)
		rightCols = append(rightCols, col)
		remainingWidth -= lipgloss.Width(col)
	}

	// spacer column to separate left- and right-aligned columns
	cols = append(cols, defaultStyle.Width(max(0, remainingWidth)).Render())
	cols = append(cols, rightCols...)
	return lipgloss.JoinHorizontal(lipgloss.Top, cols...)
}
