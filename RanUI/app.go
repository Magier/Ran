package main

import (
	"context"

	ran "github.com/Magier/Ran/core"
	"github.com/wailsapp/wails/v2/pkg/runtime"
)

// App struct
type App struct {
	ctx context.Context
	ran *ran.Ran
}

// NewApp creates a new App application struct
func NewApp() *App {
	return &App{}
}

// startup is called when the app starts. The context is saved
// so we can call the runtime methods
func (a *App) startup(ctx context.Context) {
	a.ctx = ctx
}

// Greet returns a greeting for the given name
func (a *App) StartEmulation(target string) bool {
	a.ran.Start(false, false, target, "../campaign_2025-03-03T06-31-16.json")
	runtime.EventsEmit(a.ctx, "terminal-echo", target)
	return true
}
