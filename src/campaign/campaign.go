package campaign

import (
	"context"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
)

type CampaignStarted struct {
}

func (c CampaignStarted) MessageName() string {
	return "campaign"
}

func onListenerReady(ctx context.Context, event domain.Event) (domain.Message, error) {
	// ev := event.(c2.ListenerReady)
	// print(fmt.Sprintf("Listener '%s' ready on port %d\n", ev.Name, ev.Port))
	return nil, nil
}

func onNewSession(ctx context.Context, event domain.Event, campaign *Campaign) error {
	ev := event.(c2.SessionStarted)
	campaign.sessions[ev.Session.Id] = ev.Session
	return nil
}

type Campaign struct {
	sessions map[string]c2.Session
	entities map[string]domain.Entity
}

func (c Campaign) GetEntityById(id string) (domain.Entity, bool) {
	e, ok := c.entities[id]
	return e, ok
}

func (c Campaign) GetEntityByName(name, ns string) (domain.Entity, bool) {
	for _, e := range c.entities {
		nsEntity, ok := e.(domain.Namespaced)
		if ok && nsEntity.GetNamespace() == ns && e.GetName() == name {
			return e, true
		}
	}
	return nil, false
}

func (c *Campaign) GetSessions() []c2.Session {
	sessions := make([]c2.Session, 0, len(c.sessions))
	for _, s := range c.sessions {
		sessions = append(sessions, s)
	}
	return sessions
}

func (c *Campaign) onNewEntity(ctx context.Context, event domain.Event) (domain.Message, error) {
	for _, entity := range event.(domain.NewEntities).Entities {
		c.entities[entity.GetId()] = entity
	}
	// TODO: reconcile new entities with existing ones
	return nil, nil
}

func StartCampaign(mb bus.MessageBus) *Campaign {
	campaign := Campaign{
		sessions: make(map[string]c2.Session),
		entities: make(map[string]domain.Entity),
	}
	mb.Subscribe(c2.ListenerReady{}, onListenerReady)
	mb.Subscribe(c2.SessionStarted{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		return nil, onNewSession(ctx, event, &campaign)
	})
	mb.Subscribe(domain.NewEntities{}, campaign.onNewEntity)

	err := mb.Publish(CampaignStarted{})
	if err != nil {
		panic(err)
	}
	return &campaign
}
