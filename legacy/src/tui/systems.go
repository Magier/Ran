package tui

import tea "github.com/charmbracelet/bubbletea"

type SystemsListModel struct {
}

func NewSystemsList() SystemsListModel {
	return SystemsListModel{}
}

func (m SystemsListModel) Init() tea.Cmd {
	return nil
}

func (m SystemsListModel) Update(msg tea.Msg) (SystemsListModel, tea.Cmd) {
	return m, nil
}

func (m SystemsListModel) View() string {
	return ""
}
