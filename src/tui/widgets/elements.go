package widgets

import tea "github.com/charmbracelet/bubbletea"

// var checkboxStyle = lipgloss.NewStyle().Foreground(lipgloss.Color("212"))

type CheckBox struct {
	value bool
	focus bool
}

func (e *CheckBox) SetValue(val bool) {
	e.value = val
}

func (e CheckBox) Update(msg tea.Msg) (CheckBox, tea.Cmd) {
	if !e.focus {
		return e, nil
	}
	switch k := msg.(type) {
	case tea.KeyMsg:
		switch k.String() {
		case " ":
			e.SetValue(!e.value)
		}
	}

	return e, nil
}

func (e CheckBox) View() string {
	if e.value {
		return " ✅ "
	} else {
		return " ❌ "
	}
}
func (e CheckBox) Value() bool {
	return e.value
}

func (e *CheckBox) Focus() tea.Cmd {
	e.focus = true
	return nil
}

func (e *CheckBox) Blur() {
	e.focus = false
	// e.Cursor.Blur()
}
