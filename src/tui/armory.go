package tui

type action struct {
	title, desc string
}

func (a action) Title() string       { return a.title }
func (a action) Description() string { return a.desc }
func (a action) FilterValue() string { return a.title }

type ArmoryModel struct {
}
