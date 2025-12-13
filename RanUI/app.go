package main

import (
	"context"
	"fmt"
	"time"

	api "github.com/Magier/Ran/api"
	ran "github.com/Magier/Ran/core"
	"github.com/Magier/Ran/domain"
	"github.com/wailsapp/wails/v2/pkg/runtime"
)

// API struct
type App struct {
	ctx context.Context
	ran *ran.Ran
	*api.API
}

// type Entitlement struct {
// 	Verbs         []string `json:"verbs"`
// 	ResourceTypes []string `json:"resourceTypes"`
// 	ResourceNames []string `json:"resourceNames"`
// 	Namespace     string   `json:"namespace"`
// }

// type TTP struct {
// 	ID          string `json:"id"`
// 	Name        string `json:"name"`
// 	Description string `json:"description"`
// 	Techniques   []string `json:"techniques"`
// 	Tactic      string `json:"tactic"`
// 	Variants     []string `json:"variant"`
// 	Parameters  string `json:"params"`
// }

type RuntimeWrapper struct{}

// EventsEmit implements api.Runtime.
func (r *RuntimeWrapper) EventsEmit(ctx context.Context, eventName string, data ...interface{}) {
	runtime.EventsEmit(ctx, eventName, data...)
}

// LogErrorf implements api.Runtime.
func (r *RuntimeWrapper) LogErrorf(ctx context.Context, format string, args ...interface{}) {
	runtime.LogErrorf(ctx, format, args...)
}

// LogInfof implements api.Runtime.
func (r *RuntimeWrapper) LogInfof(ctx context.Context, format string, args ...interface{}) {
	runtime.LogInfof(ctx, format, args...)
}

func (r *RuntimeWrapper) LogInfo(ctx context.Context, msg string) {
	runtime.LogInfo(ctx, msg)
}

func (r *RuntimeWrapper) LogError(ctx context.Context, msg string) {
	runtime.LogError(ctx, msg)
}

// NewApp creates a new App application struct
func NewApp() *App {
	var runtimeWrapper = &RuntimeWrapper{}
	r := ran.InitRan("", "armory/")
	a := &App{ran: &r, API: api.NewAPI(&r, runtimeWrapper)}
	return a
}

func (a *App) GetArmory() []domain.TTP {
	return a.ran.Armory.GetTTPs()
}

// func ignoreNestedAttributes(groups []string, a slog.Attr) slog.Attr {
// 	if a.Key == slog.TimeKey ||
// 		a.Key == slog.LevelKey ||
// 		a.Key == slog.MessageKey {
// 		return slog.Attr{}
// 	}
// 	return a
// }

// func NewLogHandler() slog.Handler {
// 	lvl := new(slog.LevelVar)
// 	lvl.Set(slog.LevelInfo)
// 	opts := &slog.HandlerOptions{
// 		AddSource:   false,
// 		ReplaceAttr: ignoreNestedAttributes,
// 		Level:       lvl,
// 	}
// 	b := &bytes.Buffer{}
// 	return &LogHandler{
// 		h:       slog.NewTextHandler(b, opts),
// 		b:       b,
// 		m:       &sync.Mutex{},
// 		program: p,
// 	}
// }

// startup is called when the app starts. The context is saved
// so we can call the runtime methods

func (a *App) startup(ctx context.Context) {
	a.SetContext(ctx)
	// lazy workaround; plan is to remove wails, so no need to make it perfect
	a.ctx = ctx

	runtime.EventsOn(ctx, "get-armory", func(data ...any) {
		runtime.EventsEmit(ctx, "armory-loaded", a.ran.Armory.GetTTPs())
	})

	runtime.LogInfo(a.ctx, "RanUI starting up")
	if err := a.ran.Start(ctx, false, ""); err != nil {
		runtime.LogError(ctx, "Failed to start Ran: "+err.Error())

		options := runtime.MessageDialogOptions{
			Title:   "RanUI startup error",
			Message: err.Error(),
			Type:    runtime.ErrorDialog,
		}
		if _, dlgErr := runtime.MessageDialog(ctx, options); dlgErr != nil {
			runtime.LogError(ctx, "Failed to show error dialog: "+dlgErr.Error())
		}
		runtime.Quit(ctx)
	}
	// a.ran.Start(false, "../campaign_2025-03-03T06-31-16.json")
}

func (a *App) SaveFlow() bool {
	now := time.Now().Format("2006-01-02T15-04-05")
	defaultFileName := fmt.Sprintf("campaign_%s.json", now)
	selection, err := runtime.SaveFileDialog(a.ctx, runtime.SaveDialogOptions{
		Title:                "Save Attack Flow",
		DefaultFilename:      defaultFileName,
		CanCreateDirectories: true,
		Filters: []runtime.FileFilter{
			{
				DisplayName: "Attack Flow (*.json)",
				Pattern:     "*.json",
			},
		},
	})
	if err != nil {
		runtime.LogErrorf(a.ctx, "Failed to open file dialog: %v", err)
	} else {
		ok, err := a.API.SaveFlow(selection)
		if err != nil {
			runtime.LogErrorf(a.ctx, "Failed to save flow to file '%s': %v", selection, err)
		}
		return ok
	}

	return false
}
