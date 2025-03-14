package main

import (
	"context"

	ran "github.com/Magier/Ran/core"
	domain "github.com/Magier/Ran/domain"
	"github.com/wailsapp/wails/v2/pkg/runtime"
)

// App struct
type App struct {
	ctx context.Context
	ran *ran.Ran
}

// NewApp creates a new App application struct
func NewApp() *App {
	r := ran.InitRan()

	a := &App{ran: &r}

	// forward all events directly to the frontend
	r.Bus.SubscribeToName(domain.ALL_EVENTS, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		runtime.EventsEmit(a.ctx, domain.ALL_EVENTS, msg.String())
		return nil, nil
	})

	return a
}

// startup is called when the app starts. The context is saved
// so we can call the runtime methods
func (a *App) startup(ctx context.Context) {
	a.ctx = ctx
}

// Greet returns a greeting for the given name
func (a *App) StartEmulation(target string) bool {
	a.ran.Start(false, false, target, "../campaign_2025-03-03T06-31-16.json")
	return true
}
