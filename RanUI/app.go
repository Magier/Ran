package main

import (
	"context"
	"encoding/json"
	"fmt"

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
		runtime.LogInfo(a.ctx, "Forwarding event to frontend: "+msg.String())

		jsonBytes, err := json.Marshal(msg)
		if err != nil {
			runtime.LogError(a.ctx, "failed to marshal event: "+err.Error())
			return nil, err
		}
		eventName := domain.CleanEventName(fmt.Sprintf("%T", msg))
		runtime.EventsEmit(a.ctx, eventName, string(jsonBytes))
		return nil, nil
	})

	return a
}

// startup is called when the app starts. The context is saved
// so we can call the runtime methods
func (a *App) startup(ctx context.Context) {
	a.ctx = ctx
	runtime.EventsOn(a.ctx, "get-armory", func(data ...any) {
		runtime.LogInfo(a.ctx, "get-armory")
		// runtime.EventsEmit(a.ctx, "armory", "armory")
		// runtime.EventsEmit(a.ctx, "armory", a.ran.Armory.GetTTPs())
		runtime.EventsEmit(a.ctx, "armory-loaded", a.ran.Armory.GetTTPs())
	})

}

// Greet returns a greeting for the given name
func (a *App) StartEmulation(target string) bool {
	a.ran.Start(false, false, target, "../campaign_2025-03-03T06-31-16.json")
	return true
}
