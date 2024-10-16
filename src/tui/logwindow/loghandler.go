package logwindow

import (
	"bytes"
	"context"
	"fmt"
	"log/slog"
	"strings"
	"sync"
	"time"

	tea "github.com/charmbracelet/bubbletea"
)

const (
	timeFormat = "[15:01:05.000]"
)

type LogMessage struct {
	Level string
	Time  time.Time
	Msg   string
}

func (m LogMessage) String() string {
	return fmt.Sprintf("%s %s %s", m.Time.Format(timeFormat), m.Level, m.Msg)
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
		Level: r.Level.String(),
		Time:  r.Time,
	})
	return err
}

func NewLogHandler(p *tea.Program) slog.Handler {
	opts := &slog.HandlerOptions{
		AddSource:   false,
		ReplaceAttr: ignoreNestedAttributes,
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
