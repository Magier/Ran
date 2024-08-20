package commandprompt

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/Magier/Ran/domain"
	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
)

type Model struct {
	cmdInput      textinput.Model
	actionSuccess bool
	failureReason string
}

type actionResponseMsg struct {
	success bool
	reason  string
}

type SendCommand struct {
	Action domain.Message
}

func NewCommandPrompt() Model {
	ti := textinput.New()
	ti.Placeholder = "Type a command..."
	ti.Blur()
	return Model{
		cmdInput:      ti,
		actionSuccess: true,
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	var cmds []tea.Cmd

	switch msg := msg.(type) {
	case actionResponseMsg:
		m.actionSuccess = msg.success
		m.failureReason = msg.reason
		m.cmdInput.SetValue("")
	case tea.KeyMsg:
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
		}
	}
	return m, tea.Batch(cmds...)
}

func (m Model) View() string {
	var s string
	s += m.cmdInput.View()
	if !m.actionSuccess {
		s += fmt.Sprintf(" %s ", m.failureReason+"\n")
	}

	return s
}

func handleUserAction(model Model, input string) tea.Cmd {
	return func() tea.Msg {
		action, args := parseCommand(input)
		// msgLevel := "info"
		var message string
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
			return SendCommand{Action: domain.StartListener{Port: port}}

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

func parseCommand(text string) (string, []string) {
	text = strings.Trim(text, "\n ")
	parts := strings.Split(text, " ")
	cmd := parts[0]
	args := parts[1:]

	return strings.ToLower(cmd), args
}

func (m *Model) Focus() {
	m.cmdInput.Focus()
}
func (m *Model) Blur() {
	m.cmdInput.Blur()
}
