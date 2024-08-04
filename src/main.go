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
)

func main() {
	defer fmt.Println("==== Ran exited ====")
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	// ctx, cancel := context.WithCancel(context.Background(), os.Interrupt)
	defer cancel()
	mb := bus.CreateMessageBus()
	c2.StartC2(ctx, mb)
	planner.StartApi(mb)
	campaign.StartCampaign(mb)
	// time.Sleep(60 * time.Second)
}
