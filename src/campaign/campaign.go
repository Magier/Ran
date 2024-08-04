package campaign

import (
	"context"
	"fmt"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal"
)

type CampaignStarted struct {
}

func (c CampaignStarted) EventName() string {
	return "campaign"
}

func onListenerReady(ctx context.Context, event domain.Event) error {
	ev := event.(c2.ListenerReady)
	print(fmt.Sprintf("Listener %s ready", ev.Name))
	return nil
}

func StartCampaign(mb bus.MessageBus) {
	mb.Subscribe(c2.ListenerReady{}, onListenerReady)

	err := mb.Publish(CampaignStarted{})
	if err != nil {
		panic(err)
	}
}
