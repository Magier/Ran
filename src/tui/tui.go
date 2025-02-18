package tui

import (
	"context"
	"fmt"
	"log/slog"
	"os"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
	armorywindow "github.com/Magier/Ran/tui/armorywindow"
	"github.com/Magier/Ran/tui/commandprompt"
	"github.com/Magier/Ran/tui/explorer"
	logwindow "github.com/Magier/Ran/tui/logwindow"
	"github.com/Magier/Ran/tui/mainwindow"
	"github.com/Magier/Ran/tui/statusbar"
	"github.com/Magier/Ran/tui/widgets"
	"github.com/charmbracelet/bubbles/help"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type StatusMsg struct {
	level   string // info, warning, error
	message string
}

func (m StatusMsg) String() string {
	return m.message
}

type Wnd uint

const (
	Nothing Wnd = iota
	ExplorerWnd
	ArmoryWnd
	LogWnd
	MainWnd
	CmdPrompt
)

type FocusableWnd interface {
	Focus()
	Blur()
}

func SetupTUI(bus bus.MessageBus, c *campaign.Campaign, a armory.Armory) *tea.Program {
	p := tea.NewProgram(initialModel(bus, c, a), tea.WithAltScreen(), tea.WithMouseCellMotion())

	logger := slog.New(logwindow.NewLogHandler(p))
	slog.SetDefault(logger)

	forwardEvent := func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		p.Send(msg)
		return nil, nil
	}

	bus.Subscribe(c2.ListenerReady{}, forwardEvent)
	bus.Subscribe(c2.ListenerStopped{}, forwardEvent)
	bus.Subscribe(c2.SessionStarted{}, forwardEvent)
	bus.Subscribe(c2.C2ConnectFailed{}, forwardEvent)
	bus.Subscribe(domain.C2Connected{}, forwardEvent)
	bus.Subscribe(domain.KnowledgeUpdated{}, forwardEvent)
	bus.Subscribe(domain.GraphRendered{}, forwardEvent)

	bus.Subscribe(domain.ErrorMsg{}, func(ctx context.Context, event domain.Message) (domain.Message, error) {
		msg := event.(domain.ErrorMsg)

		if msg.Level == domain.LevelFatal {
			p.Send(widgets.ShowModalMsg{
				HideRest: true,
				Text:     msg.Msg,
				Actions: []widgets.ModalAction{
					// {Label: "Retry", Action: retryCmd},
					{Label: "Quit", Action: func(map[string]string) tea.Cmd { return tea.Quit }},
				},
			})
		} else {
			p.Send(logwindow.LogMessage{Level: msg.Level, Msg: msg.Msg})
		}
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
	bus        bus.MessageBus
	armory     armorywindow.Model
	explorer   explorer.Model
	mainWindow mainwindow.Model
	cmdPrompt  commandprompt.Model
	statusBar  statusbar.Model
	logWindow  logwindow.Model
	focusedWnd Wnd
	windows    map[Wnd]FocusableWnd
	campaign   *campaign.Campaign
	keymap     keymap
	help       help.Model
	modal      widgets.ModalModel
	width      int
	height     int
}

func initialModel(bus bus.MessageBus, c *campaign.Campaign, a armory.Armory) model {
	const logWndHeight = 10

	explorer := explorer.NewExplorer(c, .3)
	mainWnd := mainwindow.NewMainWindow(c, .45, logWndHeight+1)
	armoryWnd := armorywindow.NewArmory(a, .25)
	cmdPrompt := commandprompt.NewCommandPrompt(.45)
	logWindow := logwindow.NewLogWindow(.45, logWndHeight)
	statusBar := statusbar.NewStatusBar(c)

	wnds := map[Wnd]FocusableWnd{
		ExplorerWnd: &explorer,
		// MainWnd:     &mainWnd,
		ArmoryWnd: &armoryWnd,
		// CmdPrompt:   &cmdPrompt,
		LogWnd: &logWindow,
	}

	focusedWnd := ExplorerWnd
	explorer.Focus()

	return model{
		bus:        bus,
		armory:     armoryWnd,
		campaign:   c,
		mainWindow: mainWnd,
		cmdPrompt:  cmdPrompt,
		logWindow:  logWindow,
		explorer:   explorer,
		windows:    wnds,
		modal:      widgets.NewModal(),
		focusedWnd: focusedWnd,
		statusBar:  statusBar,
		help:       help.New(),
		keymap:     setupKeymap(),
	}
}
func (m model) Init() tea.Cmd {
	return nil
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmds []tea.Cmd
	var cmd tea.Cmd

	if m, ok := msg.(tea.WindowSizeMsg); ok {
		// adjust available hight to account for statusbar
		msg = tea.WindowSizeMsg{Width: m.Width, Height: m.Height - 1}
	}

	m.modal, cmd = m.modal.Update(msg)
	cmds = append(cmds, cmd)

	m.explorer, cmd = m.explorer.Update(msg)
	m.windows[ExplorerWnd] = &m.explorer
	cmds = append(cmds, cmd)

	m.armory, cmd = m.armory.Update(msg)
	m.windows[ArmoryWnd] = &m.armory
	cmds = append(cmds, cmd)

	m.cmdPrompt, cmd = m.cmdPrompt.Update(msg)
	m.windows[CmdPrompt] = &m.cmdPrompt
	cmds = append(cmds, cmd)

	m.mainWindow, cmd = m.mainWindow.Update(msg)
	m.windows[MainWnd] = &m.mainWindow
	cmds = append(cmds, cmd)

	m.statusBar, cmd = m.statusBar.Update(msg)
	cmds = append(cmds, cmd)

	m.logWindow, cmd = m.logWindow.Update(msg)
	m.windows[LogWnd] = &m.logWindow
	cmds = append(cmds, cmd)

	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
	case commandprompt.SendCommand:
		err := m.bus.Publish(msg.Action)
		if err != nil {
			slog.Error(fmt.Sprintf("Error sending command to msg bus!!: %v\n", err))
		}
	case armorywindow.ActionSelected:
		action := msg.Action
		cmds = append(cmds, m.handleActionSelection(action)...)
	case domain.StartC2:
		err := m.bus.Publish(msg)
		if err != nil {
			slog.Error("Error sending command to msg bus!!", "", err.Error())
		}
	case tea.KeyMsg:
		switch msg.Type {
		case tea.KeyCtrlC:
			return m, tea.Quit
		case tea.KeyTab:
			m.FocusNextWnd()
		case tea.KeyShiftTab:
			m.FocusPreviousWnd()
		case tea.KeyEscape:
			m.focusedWnd = Nothing
		// default:
		// 	return handleKeyMsg(m, msg)
		default:
			switch msg.String() {
			case "p":
				err := m.bus.Publish(domain.PrintGraph{CommandImpl: domain.NewCmd()})
				if err != nil {
					slog.Error(fmt.Sprintf("Error sending command to msg bus!!: %v\n", err))
				}
			case "a":
				m.focusWindow(ArmoryWnd)
			case "e":
				m.focusWindow(ExplorerWnd)
			case ":":
				m.focusWindow(CmdPrompt)
			case "L":
				m.focusWindow(LogWnd)
			}
		}
	case widgets.ShowModalMsg:
		m.modal.SetContent(msg.Title, msg.Text, msg.Fields, msg.Actions)
		m.modal.Show(true)
		// remove focus from other window, to prevent propagating msgs in the background of the modal
		oldWnd, ok := m.windows[m.focusedWnd]
		if ok {
			oldWnd.Blur()
		}
	case widgets.CloseModalMsg:
		m.modal.Hide()
	}
	return m, tea.Batch(cmds...)
}

func (m model) View() string {
	var s string

	var modalContent string
	if m.modal.IsVisible {
		modalContent = m.modal.View()
		if m.modal.HideRest {
			return modalContent
		}
	}

	explorerView := m.explorer.View()
	mainView := m.mainWindow.View()
	logView := m.logWindow.View()
	var cmdPromptView = ""
	if m.focusedWnd == CmdPrompt {
		cmdPromptView = m.cmdPrompt.View()
	}
	_ = cmdPromptView
	armoryView := m.armory.View()
	statusBar := m.statusBar.View()

	center := lipgloss.JoinVertical(lipgloss.Left,
		mainView,
		logView,
		// cmdPromptView,
	)

	s = lipgloss.JoinVertical(lipgloss.Left,
		lipgloss.JoinHorizontal(lipgloss.Top,
			explorerView,
			center,
			armoryView),
		statusBar,
	)

	if modalContent != "" {
		s = lipgloss.Place(m.width, m.height,
			lipgloss.Center, lipgloss.Center,
			modalContent,
			// 	// lipgloss.WithWhitespaceChars("猫咪"),
			// lipgloss.WithWhitespaceForeground(subtle),
		)
	}

	return s
}

func (m *model) FocusNextWnd() {
	id := m.focusedWnd + 1
	if m.focusedWnd >= LogWnd {
		id = ExplorerWnd
	}
	m.focusWindow(id)
}

func (m *model) FocusPreviousWnd() {
	id := m.focusedWnd - 1
	if m.focusedWnd == ExplorerWnd {
		id = LogWnd
	}
	m.focusWindow(id)
}

func (m *model) focusWindow(id Wnd) {
	oldWnd, ok := m.windows[m.focusedWnd]
	if ok {
		oldWnd.Blur()
	}
	m.focusedWnd = id
	m.windows[m.focusedWnd].Focus()
}

func (m model) handleActionSelection(action armorywindow.Action) []tea.Cmd {
	cmds := []tea.Cmd{}

	targetID := m.explorer.GetSelectedEntity()
	cmd := domain.ActionSelected{
		ActionID: action.ID,
		TargetID: targetID,
	}

	sendAction := func(fields map[string]string) {
		cmd.Args = fields
		err := m.bus.Publish(cmd)
		if err != nil {
			slog.Error(fmt.Sprintf("Error sending command to msg bus!!: %v\n", err))
		}
		m.focusWindow(ExplorerWnd)
	}

	// if the action has parameters, show the modal to specify the arguments
	// otherwise, send of the aciton
	if len(action.Params) > 0 {
		modalActions := []widgets.ModalAction{
			{Label: "Run", Action: func(fields map[string]string) tea.Cmd {
				sendAction(fields)
				return nil
			}},
			{Label: "Cancel", Action: func(map[string]string) tea.Cmd { return nil }},
		}

		resMsg := widgets.ShowModalMsg{
			HideRest: true,
			Title:    action.Name,
			Text:     action.Desc,
			Fields:   action.GetFormFields(),
			Type:     widgets.Confirm,
			Actions:  modalActions,
		}
		cmds = append(cmds, func() tea.Msg { return resMsg })
	} else {
		sendAction(nil)
	}
	return cmds
}
