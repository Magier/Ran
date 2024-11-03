package logwindow

import (
	"bytes"
	"context"
	"fmt"
	"log/slog"
	"strings"
	"sync"
	"time"

	"github.com/Magier/Ran/domain"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

const (
	timeFormat  = "[15:01:05.000]"
	minLogLevel = slog.LevelDebug
)

type LogMessage struct {
	Level domain.ErrorLevel
	Time  time.Time
	Msg   string
}

func (m LogMessage) String() string {
	var symbol string

	// TODO: use constants here
	switch m.Level {
	case domain.LevelDebug:
		symbol = "🛠️ "
	case domain.LevelInfo:
		symbol = "I "
	case domain.LevelWarn:
		symbol = "⚠️"
	case domain.LevelError:
		symbol = "🚨"
	}

	return fmt.Sprintf("%s %s %s", symbol, m.Time.Format(timeFormat), m.Msg)
}
func (m LogMessage) GetColor() lipgloss.Color {
	switch m.Level {
	case domain.LevelInfo, domain.LevelDebug:
		return lipgloss.Color("#abcabc")
	default:
		return lipgloss.Color("#aaaaaa")
	}
}

type LogHandler struct {
	h       slog.Handler
	b       *bytes.Buffer
	program *tea.Program
	m       *sync.Mutex
}

func (h *LogHandler) Enabled(ctx context.Context, level slog.Level) bool {
	return h.h.Enabled(ctx, level)
}

func (h *LogHandler) WithAttrs(attrs []slog.Attr) slog.Handler {
	return &LogHandler{h: h.h.WithAttrs(attrs), program: h.program, b: h.b, m: h.m}
}
func (h *LogHandler) WithGroup(name string) slog.Handler {
	return &LogHandler{h: h.h.WithGroup(name), program: h.program, b: h.b, m: h.m}
}
func (h *LogHandler) Handle(ctx context.Context, r slog.Record) error {
	// use TextHandler to format the log message and extract it from the our own io.Writer
	h.m.Lock()
	defer func() {
		h.b.Reset()
		h.m.Unlock()
	}()

	err := h.h.Handle(ctx, r)
	attrs := strings.TrimSpace(h.b.String())

	go h.program.Send(LogMessage{
		Msg:   r.Message + " " + attrs,
		Level: domain.ErrorLevel(r.Level.String()),
		Time:  r.Time,
	})
	return err
}

func NewLogHandler(p *tea.Program) slog.Handler {
	lvl := new(slog.LevelVar)
	lvl.Set(minLogLevel)
	opts := &slog.HandlerOptions{
		AddSource:   false,
		ReplaceAttr: ignoreNestedAttributes,
		Level:       lvl,
	}
	b := &bytes.Buffer{}
	return &LogHandler{
		h:       slog.NewTextHandler(b, opts),
		b:       b,
		m:       &sync.Mutex{},
		program: p,
	}
}

func ignoreNestedAttributes(groups []string, a slog.Attr) slog.Attr {
	if a.Key == slog.TimeKey ||
		a.Key == slog.LevelKey ||
		a.Key == slog.MessageKey {
		return slog.Attr{}
	}
	return a
}
