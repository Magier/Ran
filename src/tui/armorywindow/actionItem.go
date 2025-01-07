package armory

import (
	"fmt"
	"io"
	"strings"

	"github.com/Magier/Ran/domain"
	tuimsg "github.com/Magier/Ran/tui/messages"
	"github.com/Magier/Ran/tui/theme"
	"github.com/charmbracelet/bubbles/list"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"github.com/charmbracelet/x/ansi"
)

var (
	// titleStyle        = lipgloss.NewStyle().MarginLeft(2)
	itemStyle         = lipgloss.NewStyle().PaddingLeft(4)
	selectedItemStyle = lipgloss.NewStyle().PaddingLeft(2).Foreground(lipgloss.Color("170"))
	paginationStyle   = list.DefaultStyles().PaginationStyle.PaddingLeft(4)
	helpStyle         = list.DefaultStyles().HelpStyle.PaddingLeft(4).PaddingBottom(1)
	quitTextStyle     = lipgloss.NewStyle().Margin(1, 0, 2, 4)

	dimmedTitle = lipgloss.NewStyle().Foreground(lipgloss.AdaptiveColor{Light: "#A49FA5", Dark: "#777777"}).Padding(0, 0, 0, 2).Faint(true)
	dimmedDesc  = dimmedTitle.Foreground(lipgloss.AdaptiveColor{Light: "#C2B8C2", Dark: "#4D4D4D"})
)

type actionItemDelegate struct {
	base      list.DefaultDelegate
	target    tuimsg.EntitySelected
	condition domain.Requirements
	// Styles list.DefaultItemStyles
}

func NewActionItemDelegate() actionItemDelegate {
	d := list.NewDefaultDelegate()
	d.ShowDescription = false
	return actionItemDelegate{
		base:      d,
		condition: domain.Requirements{},
	}
}

func (d actionItemDelegate) Height() int  { return 3 }
func (d actionItemDelegate) Spacing() int { return 1 }
func (d actionItemDelegate) Update(msg tea.Msg, model *list.Model) tea.Cmd {
	switch msg := msg.(type) {
	case tuimsg.EntitySelected:
		d.target = msg
		d.condition.AccessLevel = d.target.AccessLevel
		d.condition.Kind = domain.IsOfKind(d.target.Kind)
		// TODO: update RBAC permission
		model.SetDelegate(d)
	case tuimsg.StateChanged:
		d.condition.State = msg.State
		model.SetDelegate(d)
	}
	return nil
}
func (d actionItemDelegate) Render(w io.Writer, m list.Model, index int, listItem list.Item) {
	var (
		title, desc  string
		matchedRunes []int
		s            = &d.base.Styles
	)
	if m.Width() <= 0 {
		// short-circuit
		return
	}
	action, ok := listItem.(Action)
	if !ok {
		return
	}
	title = action.Title()
	desc = action.Description()

	// Prevent text from exceeding list width
	textwidth := m.Width() - s.NormalTitle.GetPaddingLeft() - s.NormalTitle.GetPaddingRight()
	title = ansi.Truncate(title, textwidth, ellipsis)
	if d.base.ShowDescription {
		var lines []string
		for i, line := range strings.Split(desc, "\n") {
			if i >= d.Height()-1 {
				break
			}
			lines = append(lines, ansi.Truncate(line, textwidth, ellipsis))
		}
		desc = strings.Join(lines, "\n")
	}

	isSatisfied := false
	if action.requirements.Satisfied(d.target, d.target.AccessLevel, d.condition.State) {
		isSatisfied = true
	}

	// Conditions
	var (
		isSelected  = index == m.Index()
		emptyFilter = m.FilterState() == list.Filtering && m.FilterValue() == ""
		isFiltered  = m.FilterState() == list.Filtering || m.FilterState() == list.FilterApplied
	)

	// if isFiltered && index < len(m.filteredItems) {
	// 	// Get indices of matched characters
	// 	matchedRunes = m.MatchesForItem(index)
	// }

	if emptyFilter || !isSatisfied {
		title = s.DimmedTitle.Render(title)
		desc = s.DimmedDesc.Render(desc)
	} else if isSelected && m.FilterState() != list.Filtering {
		if isFiltered {
			// Highlight matches
			unmatched := s.SelectedTitle.Inline(true)
			matched := unmatched.Inherit(s.FilterMatch)
			title = lipgloss.StyleRunes(title, matchedRunes, matched, unmatched)
		}
		title = s.SelectedTitle.Render(title)
		desc = s.SelectedDesc.Render(desc)
	} else {
		if isFiltered {
			// Highlight matches
			unmatched := s.NormalTitle.Inline(true)
			matched := unmatched.Inherit(s.FilterMatch)
			title = lipgloss.StyleRunes(title, matchedRunes, matched, unmatched)
		}
		title = s.NormalTitle.Render(title)
		desc = s.NormalDesc.Render(desc)
	}

	fmt.Fprintf(w, "%s", title)
	if d.base.ShowDescription && action.Description() != "" {
		fmt.Fprintf(w, "\n%s", desc)
	}
	requirementsLine := renderRequirementBadges(action.requirements, d.condition, s.NormalTitle)
	if requirementsLine != "" {
		fmt.Fprintf(w, "\n%s", requirementsLine)
	}
}

func renderRequirementBadges(r domain.Requirements, cond domain.Requirements, s lipgloss.Style) string {
	badges := make([]string, 0)
	// use the same indentation, but ensure background is only set on the actual badge content
	p := s.GetPaddingLeft()
	badgeStyle := s.PaddingLeft(0).MarginLeft(p).Foreground(theme.NegativeColor)

	checks := []struct {
		enforce   bool
		satisfied bool
		label     string
	}{
		{r.AccessLevel.IsSet(), cond.AccessLevel.Satisfies(r.AccessLevel), r.AccessLevel.String()},
		{r.Kind.IsSet(), cond.Kind.Satisfies(r.Kind), "is " + string(r.Kind)},
		{r.RbacPermission.IsSet(), cond.RbacPermission == r.RbacPermission, "can " + string(r.RbacPermission)},
		{r.Exists.IsSet(), cond.State.Satisfies(r.Exists), "∃ " + string(r.Exists)},
	}

	for _, check := range checks {
		if !check.enforce {
			continue
		}

		s := badgeStyle
		if check.satisfied {
			s = s.Foreground(theme.PositiveColor)
		}
		badges = append(badges, s.Render(check.label))
	}
	return strings.Join(badges, ", ")
}
