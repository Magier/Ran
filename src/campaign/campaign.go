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

func (c CampaignStarted) MessageName() string {
	return "campaign"
}

func onListenerReady(ctx context.Context, event domain.Event) error {
	ev := event.(c2.ListenerReady)
	print(fmt.Sprintf("Listener '%s' ready on port %d\n", ev.Name, ev.Port))
	return nil
}

func onNewSession(ctx context.Context, event domain.Event, campaign *Campaign) error {
	ev := event.(c2.SessionStarted)
	print(fmt.Sprintf("New session '%s'\n", ev.Session.Id))
	campaign.sessions[ev.Session.Id] = ev.Session
	return nil
}

type Campaign struct {
	sessions map[string]c2.Session
}

func (c *Campaign) GetSessions() []c2.Session {
	sessions := make([]c2.Session, 0, len(c.sessions))
	for _, s := range c.sessions {
		sessions = append(sessions, s)
	}
	return sessions
}

func StartCampaign(mb bus.MessageBus) *Campaign {
	campaign := Campaign{}
	mb.Subscribe(c2.ListenerReady{}, onListenerReady)
	mb.Subscribe(c2.SessionStarted{}, func(ctx context.Context, event domain.Event) error {
		return onNewSession(ctx, event, &campaign)
	})

	err := mb.Publish(CampaignStarted{})
	if err != nil {
		panic(err)
	}
	return &campaign
}
