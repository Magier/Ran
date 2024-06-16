package main

import (
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	bus "github.com/Magier/Ran/internal"
	"github.com/Magier/Ran/planner"
)

func main() {
	mb := bus.CreateMessageBus()
	c2.StartC2(mb)
	planner.StartApi(mb)
	campaign.StartCampaign(mb)
}
