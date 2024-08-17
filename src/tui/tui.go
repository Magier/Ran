package tui

import (
	"context"
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal"
	"github.com/Magier/Ran/tui/explorer"
	logwindow "github.com/Magier/Ran/tui/log_window"
	"github.com/charmbracelet/bubbles/help"
	"github.com/charmbracelet/bubbles/list"
	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

var (
	statusNugget = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#FFFDF5")).
			Padding(0, 1)

	statusBarStyle = lipgloss.NewStyle().
			Foreground(lipgloss.AdaptiveColor{Light: "#343433", Dark: "#C1C6B2"}).
			Background(lipgloss.AdaptiveColor{Light: "#D9DCCF", Dark: "#353533"})

	statusStyle = lipgloss.NewStyle().
			Inherit(statusBarStyle).
			Foreground(lipgloss.Color("#FFFDF5")).
			Background(lipgloss.Color("#FF5F87")).
			Padding(0, 1).
			MarginRight(1)

	encodingStyle = statusNugget.
			Background(lipgloss.Color("#A550DF")).
			Align(lipgloss.Right)

	statusText = lipgloss.NewStyle().Inherit(statusBarStyle)

	fishCakeStyle = statusNugget.Background(lipgloss.Color("#6124DF"))

	// Page.

	docStyle = lipgloss.NewStyle().Padding(1, 2, 1, 2)
)

type statusMsg struct {
	level   string // info, warning, error
	message string
}

func (m statusMsg) String() string {
	return m.message
}

func SetupTUI(bus bus.MessageBus, c *campaign.Campaign) *tea.Program {
	p := tea.NewProgram(initialModel(bus, c), tea.WithAltScreen())

	forwardEvent := func(ctx context.Context, event domain.Event) (domain.Message, error) {
		p.Send(event)
		return nil, nil
	}

	bus.Subscribe(c2.ListenerReady{}, forwardEvent)
	bus.Subscribe(c2.SessionStarted{}, forwardEvent)

	bus.Subscribe(domain.ErrorMsg{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		msg := event.(domain.ErrorMsg)
		p.Send(statusMsg{level: string(msg.Level), message: msg.Msg})
		return nil, nil
	})

	return p
}

func RunTUI(p *tea.Program) {
	if _, err := p.Run(); err != nil {
		fmt.Printf("Alas, there's been an error: %v", err)
		os.Exit(1)
	}
}

type model struct {
	bus bus.MessageBus
	// choices  []string // items on the to-do list
	actions       list.Model // Armory
	explorer      explorer.Model
	cmdInput      textinput.Model
	statusBar     StatusBarModel
	actionSuccess bool
	failureReason string
	campaign      *campaign.Campaign
	keymap        keymap
	help          help.Model
	logWindow     logwindow.Model
	height        int
	width         int
	// cursor   int              // which to-do list item our cursor is pointing at
	// selected map[int]struct{}, // which to-do items are selected
}

func initialModel(bus bus.MessageBus, c *campaign.Campaign) model {
	const numLogLines = 7

	actions := []list.Item{
		action{title: "Get Environment Variables", desc: "EnvVars can have secrets or interesting configurations"},
		action{title: "Nutella", desc: "It's good on toast"},
	}

	armoryModel := list.New(actions, list.NewDefaultDelegate(), 0, 0)
	armoryModel.Title = "Armory"

	ti := textinput.New()
	ti.Placeholder = "Type a command..."
	ti.Focus()

	logWindow := logwindow.New(numLogLines)

	subtle := lipgloss.AdaptiveColor{Light: "#D9DCCF", Dark: "#383838"}
	highlight := lipgloss.AdaptiveColor{Light: "#874BFD", Dark: "#7D56F4"}
	// special := lipgloss.AdaptiveColor{Light: "#43BF6D", Dark: "#73F59F"}
	color := ColorConfig{
		Foreground: highlight,
		Background: subtle,
	}

	statusBar := NewStatusBar(color, color, color, color)
	statusBar.FirstColumn = "Ran"
	statusBar.SecondColumn = "Waiting ..."

	e := explorer.NewModel()

	return model{
		bus:       bus,
		actions:   armoryModel,
		campaign:  c,
		cmdInput:  ti,
		logWindow: logWindow,
		explorer:  e,
		statusBar: statusBar,
		help:      help.New(),
		keymap:    setupKeymap(),
		height:    40,
		width:     120,
	}
}
func (m model) Init() tea.Cmd {
	return nil
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmds []tea.Cmd

	newExplorerModel, cmd := m.explorer.Update(msg)
	m.explorer = newExplorerModel
	cmds = append(cmds, cmd)

	newStatusModel, cmd := m.statusBar.Update(msg)
	m.statusBar = newStatusModel
	cmds = append(cmds, cmd)

	newLogModel, cmd := m.logWindow.Update(msg)
	m.logWindow = newLogModel
	cmds = append(cmds, cmd)

	switch msg := msg.(type) {
	case statusMsg:
		newLogModel, cmd := m.logWindow.Update(msg)
		m.logWindow = newLogModel
		cmds = append(cmds, cmd)
		// m.logWindow.AddLine(msg.message)
	case actionResponseMsg:
		m.actionSuccess = msg.success
		m.failureReason = msg.reason
		m.cmdInput.SetValue("")
	// case c2.ListenerReady:
	// newLogModel, logCmd := m.logWindow.Update(msg)
	// m.logWindow = newLogModel

	// newSystemModel, sysCmd := m.systemsList.Update(msg)
	// m.systemsList = newSystemModel

	// cmds = append(cmds, logCmd, sysCmd)
	case tea.WindowSizeMsg:
		m.height = msg.Height
		m.width = msg.Width
	case tea.KeyMsg:
		switch msg.Type {
		case tea.KeyCtrlC:
			return m, tea.Quit
		}

		// 'a' -> focus armory (it's filter function)
		// 'e' -> focus explorer
		// 'l' -> focus log
		// tab -> focus next
		// shift+tab -> focus previous
		// 'h' -> show help of focus component

		// check what is focues - is it the cmdInput?
		if m.cmdInput.Focused() {
			m.actionSuccess = true
			m.failureReason = ""
			switch msg.Type {
			// case tea.KeyEsc:
			case tea.KeyEnter:
				inputCmd := m.cmdInput.Value()
				cmd := handleUserAction(m, inputCmd)
				cmds = append(cmds, cmd)
			default:
				newCmdInput, cmd := m.cmdInput.Update(msg)
				m.cmdInput = newCmdInput
				cmds = append(cmds, cmd)
			}
		} else {
			return handleKeyMsg(m, msg)
		}
	}
	return m, tea.Batch(cmds...)
}

func handleUserAction(model model, input string) tea.Cmd {
	return func() tea.Msg {
		action, args := parseCommand(input)
		// msgLevel := "info"
		message := ""
		success := true
		switch action {
		case "listen":
			port := 1337
			if len(args) > 0 {
				var err error
				port, err = strconv.Atoi(args[0])
				if err != nil {
					// msgLevel = "warn"
					port = 1337
					message = fmt.Sprintf("Invalid port: '%s' using default port %d", args[0], port)
				}
			}
			err := model.bus.Publish(domain.StartListener{Port: port})
			if err != nil {
				// msgLevel = "error"
				message = "Failed to start listener"
			}
		// 		case "sessions":
		// 			// TODO: listen sessions from c2
		// 			sessions := camp.GetSessions()

		// 			if len(sessions) == 0 {
		// 				fmt.Println("No sessions active")
		// 			} else {
		// 				for _, s := range sessions {
		// 					fmt.Printf("Session: %s\n", s.Id)
		// 				}
		default:
			success = false
			message = fmt.Sprintf("Unknown command: '%s'", action)
		}
		return actionResponseMsg{success: success, reason: message}
		// return statusMsg{level: msgLevel, message: message}
	}
}

type actionResponseMsg struct {
	success bool
	reason  string
}

func (m model) View() string {
	var s string

	s += m.explorer.View()

	s += m.logWindow.View()

	s += m.cmdInput.View()
	if !m.actionSuccess {
		s += fmt.Sprintf(" %s ", m.failureReason+"\n")
	}

	filledLines := 5 // len of explorer + logWindow + cmdInput

	// fill screen to align rest at the bototm
	for i := 0; i < m.height-filledLines; i++ {
		s += "\n"
	}

	s += m.statusBar.View()

	// s += m.actions.View()
	// s += docStyle.Render(m.actions.View())
	return s
}

func parseCommand(text string) (string, []string) {
	text = strings.Trim(text, "\n ")
	parts := strings.Split(text, " ")
	cmd := parts[0]
	args := parts[1:]

	return strings.ToLower(cmd), args
}
