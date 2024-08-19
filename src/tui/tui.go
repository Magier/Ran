package tui

import (
	"context"
	"fmt"
	"os"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal"
	"github.com/Magier/Ran/tui/armory"
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
	MainWnd
	ArmoryWnd
	CmdPrompt
)

type FocusableWnd interface {
	Focus()
	Blur()
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
		p.Send(StatusMsg{level: string(msg.Level), message: msg.Msg})
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
	armory     armory.Model // Armory
	explorer   explorer.Model
	mainWindow mainwindow.Model
	cmdPrompt  commandprompt.Model
	statusBar  statusbar.Model
	focusedWnd Wnd
	windows    map[Wnd]FocusableWnd
	campaign   *campaign.Campaign
	keymap     keymap
	help       help.Model
	logWindow  logwindow.Model
	height     int
	width      int
	// cursor   int              // which to-do list item our cursor is pointing at
	// selected map[int]struct{}, // which to-do items are selected
}

func initialModel(bus bus.MessageBus, c *campaign.Campaign) model {
	const numLogLines = 7

	e := explorer.NewExplorer()
	mainWnd := mainwindow.NewMainWindow()
	armory := armory.NewAmory()
	cmdPrompt := commandprompt.NewCommandPrompt()
	logWindow := logwindow.NewLogWindow(numLogLines)
	statusBar := statusbar.NewStatusBar()
	focusedWnd := ArmoryWnd

	wnds := map[Wnd]FocusableWnd{
		ExplorerWnd: e,
		MainWnd:     mainWnd,
		ArmoryWnd:   armory,
		CmdPrompt:   cmdPrompt,
	}

	return model{
		bus: bus,
		// actions:   armory,
		armory:     armory,
		campaign:   c,
		mainWindow: mainWnd,
		cmdPrompt:  cmdPrompt,
		logWindow:  logWindow,
		explorer:   e,
		windows:    wnds,
		focusedWnd: focusedWnd,
		statusBar:  statusBar,
		help:       help.New(),
		keymap:     setupKeymap(),
		height:     40,
		width:      120,
	}
}
func (m model) Init() tea.Cmd {
	return nil
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmds []tea.Cmd
	var cmd tea.Cmd

	m.explorer, cmd = m.explorer.Update(msg)
	cmds = append(cmds, cmd)

	m.statusBar, cmd = m.statusBar.Update(msg)
	cmds = append(cmds, cmd)

	m.logWindow, cmd = m.logWindow.Update(msg)
	cmds = append(cmds, cmd)

	m.armory, cmd = m.armory.Update(msg)
	cmds = append(cmds, cmd)

	m.cmdPrompt, cmd = m.cmdPrompt.Update(msg)
	cmds = append(cmds, cmd)

	switch msg := msg.(type) {
	case StatusMsg:
		newLogModel, cmd := m.logWindow.Update(msg)
		m.logWindow = newLogModel
		cmds = append(cmds, cmd)
		// m.logWindow.AddLine(msg.message)
	// case c2.ListenerReady:
	// newLogModel, logCmd := m.logWindow.Update(msg)
	// m.logWindow = newLogModel

	// newSystemModel, sysCmd := m.systemsList.Update(msg)
	// m.systemsList = newSystemModel

	// cmds = append(cmds, logCmd, sysCmd)
	case tea.WindowSizeMsg:
		m.height = msg.Height
		m.width = msg.Width

	case commandprompt.SendCommand:
		err := m.bus.Publish(msg.Action)
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
		default:
			return handleKeyMsg(m, msg)
		}

		// 'a' -> focus armory (it's filter function)
		// 'e' -> focus explorer
		// 'l' -> focus log
		// ':' -> show commandPrompt
		// tab -> focus next
		// shift+tab -> focus previous
		// 'h' -> show help of focus component

		// check what is focues - is it the cmdInput?
		// if m.cmdInput.Focused() {
		// } else {
		// }
	}
	return m, tea.Batch(cmds...)
}

func (m model) View() string {
	var s string

	s += lipgloss.JoinVertical(lipgloss.Left,
		lipgloss.JoinHorizontal(lipgloss.Top,
			m.explorer.View(),
			lipgloss.JoinVertical(lipgloss.Left,
				m.mainWindow.View(),
				m.logWindow.View(),
				m.cmdPrompt.View(),
			),
			m.armory.View()),
		m.statusBar.View(),
	)
	return s
}

func (m *model) FocusNextWnd() {
	oldWnd, ok := m.windows[m.focusedWnd]
	if ok {
		oldWnd.Blur()
	}

	m.focusedWnd += 1
	if m.focusedWnd > CmdPrompt {
		m.focusedWnd = ExplorerWnd
	}

	newWnd := m.windows[m.focusedWnd]
	newWnd.Focus()
}
func (m *model) FocusPreviousWnd() {
	oldWnd, ok := m.windows[m.focusedWnd]
	if ok {
		oldWnd.Blur()
	}
	if m.focusedWnd == ExplorerWnd {
		m.focusedWnd = CmdPrompt
	} else {
		m.focusedWnd -= 1
	}
	newWnd := m.windows[m.focusedWnd]
	newWnd.Focus()
}
