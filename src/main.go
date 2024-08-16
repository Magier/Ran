package main

import (
	"context"
	"os"
	"os/signal"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	bus "github.com/Magier/Ran/internal"
	"github.com/Magier/Ran/planner"
	tui "github.com/Magier/Ran/tui"
)

func main() {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	// ctx, cancel := context.WithCancel(context.Background(), os.Interrupt)
	defer cancel()
	mb := bus.CreateMessageBus()
	c := campaign.StartCampaign(mb)
	ui := tui.SetupTUI(mb, c)
	c2.StartC2(ctx, mb)
	planner.StartApi(mb)

	go mb.HandleEvents(ctx)
	// TODO maybe switch between TUI and web-UI (start frontend as well?)
	tui.RunTUI(ui)
}
