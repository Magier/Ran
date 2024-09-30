package campaign

import (
	"context"
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
)

type CampaignStarted struct {
}

func (c CampaignStarted) MessageName() string {
	return "campaign"
}

func onNewSession(ctx context.Context, event domain.Event, campaign *Campaign) error {
	ev := event.(c2.SessionStarted)
	campaign.sessions[ev.Session.Id] = ev.Session
	return nil
}

type Campaign struct {
	listeners map[string]uint
	sessions  map[string]c2.Session
	entities  map[string]domain.Entity
}

func (c *Campaign) GetEntityById(id string) (domain.Entity, bool) {
	e, ok := c.entities[id]
	return e, ok
}

func (c *Campaign) GetEntityByName(name, ns string) (domain.Entity, bool) {
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
func (c *Campaign) onListenerReady(ctx context.Context, event domain.Event) (domain.Message, error) {
	ev := event.(c2.ListenerReady)
	c.listeners[ev.Name] = ev.Port
	return nil, nil
}

func StartCampaign(mb bus.MessageBus) *Campaign {
	campaign := &Campaign{
		sessions:  make(map[string]c2.Session),
		entities:  make(map[string]domain.Entity),
		listeners: make(map[string]uint),
	}
	mb.Subscribe(c2.ListenerReady{}, campaign.onListenerReady)
	mb.Subscribe(c2.SessionStarted{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		return nil, onNewSession(ctx, event, campaign)
	})
	mb.Subscribe(domain.NewEntities{}, campaign.onNewEntity)

	err := mb.Publish(CampaignStarted{})
	if err != nil {
		panic(err)
	}
	return campaign
}

func (c Campaign) InflateActionTemplate(action domain.Message, targetId string) (domain.Message, error) {
	tmpl, ok := action.(domain.Templater)
	if ok {
		template := tmpl.GetTemplate()
		// do thing
		if strings.Contains(template, "$LISTENER_PORT") {
			// get listener port from campaign
			c.GetSessions()
			template = strings.Replace(template, "$LISTENER_PORT", "1337", -1)
		}
		if strings.Contains(template, "$LISTENER") {
			// get listener from campaign
			c.GetSessions()
			template = strings.Replace(template, "$LISTENER", "arstarst", -1)
		}

		tmpl.SetGroundedString(template)
	}

	// check if it is targeted
	t, ok := action.(domain.Targeter)
	if ok {
		e, ok := c.entities[targetId]
		if ok {
			t.SetTarget(e)
		}
	}

	return action, nil
}
