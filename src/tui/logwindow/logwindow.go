package logwindow

import (
	"fmt"
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/tui/theme"
	"github.com/charmbracelet/bubbles/viewport"
	tea "github.com/charmbracelet/bubbletea"

	"github.com/charmbracelet/lipgloss"
)

type Model struct {
	focused      bool
	nummLines    int
	lines        []string
	style        lipgloss.Style
	viewport     viewport.Model
	width        float32
	height       float32
	ready        bool
	scrollOffset int
}

func NewLogWindow(numLogLines int, width float32, height float32) Model {
	var style = lipgloss.NewStyle().
		Bold(true).
		// Foreground(lipgloss.Color("#7D56F4")).
		// Background(lipgloss.Color("#FAFAFA")).
		PaddingLeft(1).
		Height(6).
		MaxHeight(6).
		Width(80).
		BorderStyle(lipgloss.RoundedBorder()).
		BorderForeground(theme.InactiveColor).
		BorderTop(true).
		BorderLeft(true).
		BorderRight(true).
		BorderBottom(true)

	return Model{
		nummLines:    numLogLines,
		focused:      true,
		lines:        make([]string, 0, numLogLines),
		style:        style,
		width:        width,
		height:       height,
		ready:        false,
		scrollOffset: 0,
	}
}

func (m *Model) AddLine(line string) {
	m.lines = append(m.lines, line)
	var s string
	for _, res := range m.lines {
		s += res + "\n"
	}
	m.viewport.SetContent(s)
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	var (
		cmd  tea.Cmd
		cmds []tea.Cmd
	)
	switch msg := msg.(type) {
	case LogMessage:
		m.AddLine(msg.String())
	case c2.ListenerReady:
		m.AddLine(msg.String())
	case c2.SessionStarted:
		m.AddLine(msg.String())
	case tea.WindowSizeMsg:
		footerHeight := lipgloss.Height(m.footerView())

		w := int(m.width * float32(msg.Width))
		h := int(m.height*float32(msg.Height)) - 1 // -1 for the statusbar and top border
		m.style = m.style.Width(w)
		m.style = m.style.Height(h)

		if !m.ready {
			m.viewport = viewport.New(w, h-footerHeight)
			m.ready = true
		} else {
			m.viewport.Width = msg.Width
			m.viewport.Height = msg.Height - footerHeight
		}
	case tea.KeyMsg:
		if m.focused {
			switch msg.String() {
			case "g": // go to the top
				m.viewport.GotoTop()
			case "G": // go to the bottom
				m.viewport.GotoBottom()
			}
		}
	}
	// Handle keyboard and mouse events in the viewport
	m.viewport, cmd = m.viewport.Update(msg)
	cmds = append(cmds, cmd)

	return m, tea.Batch(cmds...)
}

func (m Model) View() string {
	var s string
	for _, res := range m.lines {
		s += res + "\n"
	}
	if !m.ready {
		return "\n  Initializing..."
	}
	s = fmt.Sprintf("%s\n%s", m.viewport.View(), m.footerView())

	if m.focused {
		activeStyle := m.style.BorderForeground(theme.PrimaryColor)
		return activeStyle.Render(s)
	} else {
		return m.style.Render(s)
	}
}

func (m *Model) Focus() {
	m.focused = true
}
func (m *Model) Blur() {
	m.focused = false
}

func (m Model) footerView() string {
	info := fmt.Sprintf("%d enries %3.f%% ", len(m.lines), m.viewport.ScrollPercent()*100)
	// info := infoStyle.Render(fmt.Sprintf("%d enries %3.f%%", len(m.lines), m.viewport.ScrollPercent()*100))
	w := m.viewport.Width - lipgloss.Width(info) - 1
	line := strings.Repeat("─", max(0, w))
	return lipgloss.JoinHorizontal(lipgloss.Center, info, line)
}
