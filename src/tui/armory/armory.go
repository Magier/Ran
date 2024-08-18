package armory

import (
	"github.com/Magier/Ran/tui/theme"
	"github.com/charmbracelet/bubbles/list"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

var style = lipgloss.NewStyle().
	Bold(true).
	// Foreground(lipgloss.Color("#7D56F4")).
	// Background(lipgloss.Color("#FAFAFA")).
	PaddingTop(1).
	PaddingLeft(1).
	Height(35).
	Width(22).
	BorderStyle(lipgloss.RoundedBorder()).
	BorderForeground(theme.PrimaryColor).
	BorderTop(true).
	BorderLeft(true).
	BorderRight(true).
	BorderBottom(true)

type Action struct {
	title, desc string
}

func (a Action) Title() string       { return a.title }
func (a Action) Description() string { return a.desc }
func (a Action) FilterValue() string { return a.title }

type Model struct {
	actions list.Model
}

func NewAmory() Model {
	actions := []list.Item{
		Action{title: "Get Environment Variables", desc: "EnvVars can have secrets or interesting configurations"},
		Action{title: "Nutella", desc: "It's good on toast"},
	}
	return Model{
		actions: list.New(actions, list.NewDefaultDelegate(), 0, 0),
	}
}

func (m Model) Init() tea.Cmd {
	return nil
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	return m, nil
}

func (m Model) View() string {
	return style.Render("Armory")
}
