package campaign

import (
	bus "github.com/Magier/Ran/internal"
)

type CampaignStarted struct {
}

func (c CampaignStarted) EventName() string {
	return "campaign"
}

func StartCampaign(mb bus.MessageBus) {
	err := mb.Publish(CampaignStarted{})
	if err != nil {
		panic(err)
	}
}
