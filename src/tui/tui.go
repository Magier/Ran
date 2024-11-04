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
	armoryWindow "github.com/Magier/Ran/tui/armoryWindow"
	"github.com/Magier/Ran/tui/commandprompt"
	"github.com/Magier/Ran/tui/explorer"
	logwindow "github.com/Magier/Ran/tui/logwindow"
	"github.com/Magier/Ran/tui/mainwindow"
	"github.com/Magier/Ran/tui/statusbar"
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
	p := tea.NewProgram(initialModel(bus, c, a), tea.WithAltScreen())

	logger := slog.New(logwindow.NewLogHandler(p))
	slog.SetDefault(logger)

	forwardEvent := func(ctx context.Context, event domain.Event) (domain.Message, error) {
		p.Send(event)
		return nil, nil
	}

	bus.Subscribe(c2.ListenerReady{}, forwardEvent)
	bus.Subscribe(c2.SessionStarted{}, forwardEvent)
	bus.Subscribe(c2.C2ConnectFailed{}, forwardEvent)
	bus.Subscribe(domain.ConnectedToExternalC2Server{}, forwardEvent)
	bus.Subscribe(domain.KnowledgeUpdated{}, forwardEvent)

	bus.Subscribe(domain.ErrorMsg{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		msg := event.(domain.ErrorMsg)
		p.Send(logwindow.LogMessage{Level: msg.Level, Msg: msg.Msg})
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
	armory     armoryWindow.Model
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
}

func initialModel(bus bus.MessageBus, c *campaign.Campaign, a armory.Armory) model {
	const logWndHeight = 10

	explorer := explorer.NewExplorer(c, .3)
	mainWnd := mainwindow.NewMainWindow(.45, logWndHeight+1)
	armoryWnd := armoryWindow.NewArmory(a, .25)
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
		// m.height = msg.Height
		// m.width = msg.Width

	case commandprompt.SendCommand:
		err := m.bus.Publish(msg.Action)
		if err != nil { // TODO: properly handle errors in UI
			fmt.Printf("Error sending command to msg bus!!: %v\n", err)
		}
	case armoryWindow.ActionSelected:
		target := m.explorer.GetSelectedEntity()

		a, err := m.campaign.InflateActionTemplate(msg.Action, target)
		if err != nil {
			fmt.Printf("Could not inflate action template: %v\n", err)
		}

		err = m.bus.Publish(a)
		// TODO: properly assemble the action by interpolating the values
		// e.g. the target of the action, parameters, etc.
		if err != nil { // TODO: properly handle errors in UI
			fmt.Printf("Error sending command to msg bus!!: %v\n", err)
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
	}
	return m, tea.Batch(cmds...)
}

func (m model) View() string {
	var s string

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
