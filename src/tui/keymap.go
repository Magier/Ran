package tui

import (
	"github.com/charmbracelet/bubbles/key"
	tea "github.com/charmbracelet/bubbletea"
)

type keymap = struct {
	next, prev, down, up, listen, quit key.Binding
}

func setupKeymap() keymap {
	return keymap{
		next: key.NewBinding(
			key.WithKeys("tab"),
			key.WithHelp("tab", "next"),
		),
		prev: key.NewBinding(
			key.WithKeys("shift+tab"),
			key.WithHelp("shift+tab", "prev"),
		),
		// down: key.NewBinding(
		// 	key.WithKeys("j", key.KeyDown),
		// 	key.WithHelp("Next", "j"),
		// ),
		// up: key.NewBinding(
		// 	key.WithKeys("k", key.KeyUp),
		// 	key.WithHelp("Previous", "k"),
		// ),
		listen: key.NewBinding(
			key.WithKeys("l"),
			key.WithHelp("Listen", "l"),
		),
		quit: key.NewBinding(
			key.WithKeys("q"),
			key.WithHelp("Quit", "q"),
		),
		// quit: key.NewBinding(
		// 	key.WithKeys("esc", "ctrl+c"),
		// 	key.WithHelp("esc", "quit"),
		// ),
		// add: key.NewBinding(
		// 	key.WithKeys("ctrl+n"),
		// 	key.WithHelp("ctrl+n", "add an editor"),
		// ),
		// remove: key.NewBinding(
		// 	key.WithKeys("ctrl+w"),
		// 	key.WithHelp("ctrl+w", "remove an editor"),
		// ),
	}
}

func handleKeyMsg(m tea.Model, msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.String() {
	// These keys should exit the program.
	case "ctrl+c":
		return m, tea.Quit
	// The "up" and "k" keys move the cursor up
	case "up", "k":
		// if m.cursor > 0 {
		// 	m.cursor--
		// }
	// The "down" and "j" keys move the cursor down
	case "down", "j":
		// if m.cursor < len(m.actions)-1 {
		// 	m.cursor++
		// }

	// The "enter" key and the spacebar (a literal space) toggle
	// the selected state for the item that the cursor is pointing at.
	case "enter", " ":
		// _, ok := m.selected[m.cursor]
		// if ok {
		// 	delete(m.selected, m.cursor)
		// } else {
		// 	m.selected[m.cursor] = struct{}{}
		// }
	case "l":
		// TODO show modal with default port filled in
		return m, nil
	}
	return m, nil
}
