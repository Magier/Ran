package statusbar

import (
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"github.com/muesli/reflow/truncate"
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

// Height represents the height of the statusbar.
const Height = 1

// ColorConfig
type ColorConfig struct {
	Foreground lipgloss.AdaptiveColor
	Background lipgloss.AdaptiveColor
}

// Model represents the properties of the statusbar.
type Model struct {
	Width              int
	Height             int
	FirstColumn        string
	SecondColumn       string
	ThirdColumn        string
	FourthColumn       string
	FirstColumnColors  ColorConfig
	SecondColumnColors ColorConfig
	ThirdColumnColors  ColorConfig
	FourthColumnColors ColorConfig
}

// NewStatusBar creates a new instance of the statusbar.
func NewStatusBar() Model {
	subtle := lipgloss.AdaptiveColor{Light: "#D9DCCF", Dark: "#383838"}
	highlight := lipgloss.AdaptiveColor{Light: "#874BFD", Dark: "#7D56F4"}
	// special := lipgloss.AdaptiveColor{Light: "#43BF6D", Dark: "#73F59F"}
	color := ColorConfig{
		Foreground: highlight,
		Background: subtle,
	}

	return Model{
		FirstColumnColors:  color,
		SecondColumnColors: color,
		ThirdColumnColors:  color,
		FourthColumnColors: color,
		FirstColumn:        "Ran",
		SecondColumn:       "Waiting ...",
	}
}

// SetSize sets the width of the statusbar.
func (m *Model) SetSize(width int) {
	m.Width = width
}

// Update updates the size of the statusbar.
func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.SetSize(msg.Width)
	}

	return m, nil
}

// SetContent sets the content of the statusbar.
func (m *Model) SetContent(firstColumn, secondColumn, thirdColumn, fourthColumn string) {
	m.FirstColumn = firstColumn
	m.SecondColumn = secondColumn
	m.ThirdColumn = thirdColumn
	m.FourthColumn = fourthColumn
}

// SetColors sets the colors of the 4 columns.
func (m *Model) SetColors(firstColumnColors, secondColumnColors, thirdColumnColors, fourthColumnColors ColorConfig) {
	m.FirstColumnColors = firstColumnColors
	m.SecondColumnColors = secondColumnColors
	m.ThirdColumnColors = thirdColumnColors
	m.FourthColumnColors = fourthColumnColors
}

// View returns a string representation of a statusbar.
func (m Model) View() string {

	// w := lipgloss.Width

	// statusKey := statusStyle.Render("STATUS")
	// encoding := encodingStyle.Render("UTF-8")
	// fishCake := fishCakeStyle.Render("🍥 Fish Cake")
	// width := 96
	// // width := pty.Window.Width
	// statusVal := statusText.
	// 	Width(width - w(statusKey) - w(encoding) - w(fishCake)).
	// 	Render("Ravishing")

	// bar := lipgloss.JoinHorizontal(lipgloss.Top,
	// 	statusKey,
	// 	statusVal,
	// 	encoding,
	// 	fishCake,
	// )

	// s += statusBarStyle.Width(width).Render(bar)

	width := lipgloss.Width

	firstColumn := lipgloss.NewStyle().
		Foreground(m.FirstColumnColors.Foreground).
		Background(m.FirstColumnColors.Background).
		Padding(0, 1).
		Height(Height).
		Render(truncate.StringWithTail(m.FirstColumn, 30, "..."))

	thirdColumn := lipgloss.NewStyle().
		Foreground(m.ThirdColumnColors.Foreground).
		Background(m.ThirdColumnColors.Background).
		Align(lipgloss.Right).
		Padding(0, 1).
		Height(Height).
		Render(m.ThirdColumn)

	fourthColumn := lipgloss.NewStyle().
		Foreground(m.FourthColumnColors.Foreground).
		Background(m.FourthColumnColors.Background).
		Padding(0, 1).
		Height(Height).
		Render(m.FourthColumn)

	secondColumn := lipgloss.NewStyle().
		Foreground(m.SecondColumnColors.Foreground).
		Background(m.SecondColumnColors.Background).
		Padding(0, 1).
		Height(Height).
		Width(m.Width - width(firstColumn) - width(thirdColumn) - width(fourthColumn)).
		Render(truncate.StringWithTail(
			m.SecondColumn,
			uint(m.Width-width(firstColumn)-width(thirdColumn)-width(fourthColumn)-3),
			"..."),
		)

	return lipgloss.JoinHorizontal(lipgloss.Top,
		firstColumn,
		secondColumn,
		thirdColumn,
		fourthColumn,
	)
}
