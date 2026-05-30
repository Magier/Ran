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

func NewMainWindow(c *campaign.Campaign, width float32, bottomOffset int) Model {
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
	case domain.FactsChanged:
		if m.entity != nil {
			entityID := m.entity.GetId()
			if e, ok := m.campaign.GetEntityById(entityID); ok {
				m.entity = e
				m.showEntityDetails = true
			} else {
				slog.Warn("TUI", "", "Failed to resolve "+entityID)
			}
		}
	case domain.GraphRendered:
		m.showEntityDetails = false
	}

	return m, nil
}

func (m Model) View() string {
	var s string
	if m.showEntityDetails {
		s = renderEntity(m.entity)
	} else {
		if imgStr, ok := getTopologyImgContent("topo.png", m.style.GetWidth()-1, m.style.GetHeight()-1); ok {
			s += imgStr
		}
	}

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

func getTopologyImgContent(path string, width, height int) (string, bool) {
	if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
		return "", false
	}

	f, err := os.Open(path)
	if err != nil {
		panic(err)
	}
	defer f.Close()
	img, err := png.Decode(f)
	if err != nil {
		panic(err)
	}

	// Set the expected size that you want:
	// dst := image.NewRGBA(image.Rect(0, 0, img.Bounds().Max.X/2, img.Bounds().Max.Y/2))
	// dst := image.NewRGBA(image.Rect(0, 0, width, height))

	// Resize:
	// draw.NearestNeighbor.Scale(dst, dst.Rect, img, img.Bounds(), draw.Over, nil)

	var buf bytes.Buffer
	err = Fprint(&buf, img, width, height)
	if err != nil {
		return "", false
	} else {
		a := buf.Bytes()[:]
		return string(a), true
	}
}

// Source: https://github.com/dolmen-go/kittyimg
func Fprint(w io.Writer, img image.Image, cols int, rows int) error {
	bounds := img.Bounds()

	// transmitAndDisplay := "a=T"
	// 32bit := f=32

	// f=32 => RGBA
	_, err := fmt.Fprintf(w, "\033_Gq=1,a=T,f=32,s=%d,v=%d,c=%d,t=d,", bounds.Dx(), bounds.Dy(), cols)
	if err != nil {
		return err
	}

	buf := make([]byte, 0, 4*4096) // Multiple of 4 (RGBA)

	// var p streamPayload
	var p zlibPayload
	p.Reset(w)

	for y := bounds.Min.Y; y < bounds.Max.Y; y++ {
		for x := bounds.Min.X; x < bounds.Max.X; x++ {
			if len(buf) == cap(buf) {
				if _, err = p.Write(buf); err != nil {
					return err
				}
				buf = buf[:0]
			}
			r, g, b, a := img.At(x, y).RGBA()
			// A color's RGBA method returns values in the range [0, 65535].
			// Shifting by 8 reduces this to the range [0, 255].
			buf = append(buf, byte(r>>8), byte(g>>8), byte(b>>8), byte(a>>8))
		}
	}

	if _, err = p.Write(buf); err != nil {
		return err
	}
	return p.Close()
}
