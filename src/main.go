package main

import (
	"context"
	"fmt"
	"os"
	"os/signal"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	bus "github.com/Magier/Ran/internal"
	"github.com/Magier/Ran/planner"
	tui "github.com/Magier/Ran/tui"
)

func main() {
	defer fmt.Println("==== Ran exited ====")
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	// ctx, cancel := context.WithCancel(context.Background(), os.Interrupt)
	defer cancel()
	mb := bus.CreateMessageBus()
	go mb.HandleEvents(ctx)
	c := campaign.StartCampaign(mb)
	c2.StartC2(ctx, mb)
	planner.StartApi(mb)

	// TODO maybe switch between TUI and web-UI (start frontend as well?)
	tui.SimpleTUI(mb, c)

}
