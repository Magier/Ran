package mainwindow

import (
	"bytes"
	"errors"
	"fmt"
	"image"
	"image/png"
	"io"
	"log/slog"
	"os"

	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	tuimsg "github.com/Magier/Ran/tui/messages"
	"github.com/Magier/Ran/tui/theme"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

type Model struct {
	campaign          *campaign.Campaign
	focused           bool
	style             lipgloss.Style
	width             float32
	bottomOffset      int
	entity            domain.Entity
	showEntityDetails bool
}

func NewMainWindow(width float32, bottomOffset int) Model {
	var style = lipgloss.NewStyle().
		Bold(true).
		// Foreground(lipgloss.Color("#7D56F4")).
		// Background(lipgloss.Color("#FAFAFA")).
		PaddingTop(1).
		PaddingLeft(1).
		Height(26).
		Width(80).
		BorderStyle(lipgloss.RoundedBorder()).
		BorderForeground(theme.InactiveColor).
		BorderTop(true).
		BorderLeft(true).
		BorderRight(true).
		BorderBottom(false)

	return Model{
		showEntityDetails: true,
		campaign:          c,
		focused:           false,
		style:             style,
		width:             width,
		bottomOffset:      bottomOffset,
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.style = m.style.Width(int(m.width * float32(msg.Width)))
		m.style = m.style.Height(msg.Height - m.bottomOffset - 1)
	case tuimsg.EntitySelected:
		if e, ok := m.campaign.GetEntityById(msg.Id); ok {
			m.entity = e
			m.showEntityDetails = true
		} else {
			slog.Warn("TUI", "", "Failed to resolve "+msg.Id)
		}
	}

	return m, nil
}

func (m Model) View() string {
	var s string
	if m.showEntityDetails {
		s = renderEntity(m.entity)
	}

	if m.focused {
		activeStyle := m.style.BorderForeground(theme.PrimaryColor)
		return activeStyle.Render(s)
	} else {
		return m.style.Render(s)
	}
}

func renderEntity(entity domain.Entity) string {
	switch e := entity.(type) {
	case domain.Pod:
		return fmt.Sprintf("Pod ID: %s\nNamespace: %s\nHostName: %s\nIP: %s\nNodeName: %s",
			e.GetId(), e.Namespace, e.HostName, e.IP, e.NodeName)

	case domain.ServiceAccount:
		return "SAAA " + e.GetId()
	case nil:
		return "-"
	}
	return entity.GetId()
}

func (m *Model) Focus() {
	m.focused = true
}
func (m *Model) Blur() {
	m.focused = false
}
