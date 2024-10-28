package campaign

import (
	"context"
	"fmt"
	"log/slog"
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
)

type CampaignStarted struct {
}

func (c CampaignStarted) String() string {
	return "campaign started"
}

func onNewSession(ctx context.Context, event domain.Event, campaign *Campaign) error {
	ev := event.(c2.SessionStarted)
	campaign.sessions[ev.Session.Id] = ev.Session
	return nil
}

type Campaign struct {
	listeners map[string]domain.Listener
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
	id := fmt.Sprintf("%s_%d", ev.Name, ev.Port)
	c.listeners[id] = domain.Listener{
		ID:         id,
		IP:         ev.IP,
		Port:       ev.Port,
		Protocol:   ev.Protocol,
		Redirector: "",
	}
	return nil, nil
}

func StartCampaign(mb bus.MessageBus) *Campaign {
	campaign := &Campaign{
		sessions:  make(map[string]c2.Session),
		entities:  make(map[string]domain.Entity),
		listeners: make(map[string]domain.Listener),
	}
	mb.Subscribe(c2.ListenerReady{}, campaign.onListenerReady)
	mb.Subscribe(c2.SessionStarted{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		return nil, onNewSession(ctx, event, campaign)
	})
	mb.Subscribe(domain.NewEntities{}, campaign.onNewEntity)
	mb.Subscribe(domain.EnvVarsExtracted{}, campaign.onEnvVarsExtracted)

	err := mb.Publish(CampaignStarted{})
	if err != nil {
		panic(err)
	}
	return campaign
}

func (c Campaign) InflateActionTemplate(action domain.Message, targetId string) (domain.Message, error) {
	// TODO: do not alter the actual template used fo multiple invocations
	tmpl, ok := action.(domain.Templater)
	if ok {
		template := tmpl.GetTemplate()
		if strings.Contains(template, "$LISTENER") {
			listener, ok := c.GetListener(domain.TCP)
			if ok {
				template = inflateListenerTemplate(listener, template)
			} else {
				slog.Info("No suitable listener found!")
			}
		}
		if strings.Contains(template, "$FILESHARE_PORT") {
			filesharePort, ok := c.GetFileshare()
			if ok {
				p := fmt.Sprint(filesharePort)
				template = strings.Replace(template, "$FILESHARE_PORT", p, -1)
			} else {
				slog.Info("No suitable fileshare found!")
			}
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

// GetListener returns the best suitable listener given the constraints
func (c Campaign) GetListener(protocol domain.Protocol) (domain.Listener, bool) {
	for _, l := range c.listeners {
		if l.Protocol == protocol || l.Protocol == domain.ANY {
			return l, true
		}
	}

	return domain.Listener{}, false
}

func (c Campaign) GetFileshare() (uint, bool) {
	// TODO: properly implement this
	return 3000, true
}

func (c *Campaign) onEnvVarsExtracted(ctx context.Context, event domain.Event) (domain.Message, error) {
	return analyzeEnvironmentVariables(event.(domain.EnvVarsExtracted))
}

func inflateListenerTemplate(listener domain.Listener, template string) string {
	// TODO: properly handle multiple protocols!
	if strings.Contains(template, "$LISTENER_PORT") {
		p := fmt.Sprint(listener.Port)
		template = strings.Replace(template, "$LISTENER_PORT", p, -1)
	}

	var dst string
	if listener.Redirector != "" {
		dst = listener.Redirector
	} else {
		dst = listener.IP.String()
	}

	template = strings.Replace(template, "$LISTENER", dst, -1)
	return template
}
