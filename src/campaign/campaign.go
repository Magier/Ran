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

func (c *Campaign) onNewSession(ev c2.SessionStarted) (domain.Message, error) {
	c.sessions[ev.Session.Id] = ev.Session
	return nil, nil
}

func (c *Campaign) onSessionClosed(ev c2.SessionClosed) (domain.Message, error) {
	_, ok := c.sessions[ev.Session.Id]
	if !ok {
		return nil, fmt.Errorf("Unknwon session '%s' could not be closed", ev.Session.Id)
	}
	delete(c.sessions, ev.Session.Id)
	return nil, nil
}

type Campaign struct {
	activeIdentity string
	listeners      map[string]domain.Listener
	sessions       map[string]c2.Session
	entities       map[string]domain.Entity
	relations      []domain.Relation
	identities     map[string]domain.Identity
}

func (c *Campaign) GetEntities() map[string]domain.Entity {
	return c.entities
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

func (c *Campaign) GetActiveIdentity() (domain.Identity, bool) {
	if c.activeIdentity == "" {
		return domain.Identity{}, false
	}
	id, ok := c.identities[c.activeIdentity]
	return id, ok
}

func (c *Campaign) GetIdentities() map[string]domain.Identity {
	return c.identities
}

func (c *Campaign) GetSessions() []c2.Session {
	sessions := make([]c2.Session, 0, len(c.sessions))
	for _, s := range c.sessions {
		sessions = append(sessions, s)
	}
	return sessions
}

func (c *Campaign) onNewFacts(ctx context.Context, event domain.Event) (domain.Message, error) {
	// TODO: properly track how many changes the update contained
	numChanges := 0
	ev := event.(domain.NewFacts)
	for _, entity := range ev.Entities {
		c.entities[entity.GetId()] = entity
		otherEntities, relations := extractRelatedEntities(c, entity)
		numChanges++
		for _, e := range otherEntities {
			c.entities[e.GetId()] = e
			numChanges++
		}

		for _, rel := range relations {
			c.relations = append(c.relations, rel)
			numChanges++
		}
	}

	for _, identity := range ev.Identities {
		// if there is no active identity, use the first encountered Id as the active oneo
		if c.activeIdentity == "" {
			c.activeIdentity = identity.Name
		}
		c.identities[identity.Name] = identity
	}

	// TODO: reconcile new entities with existing ones
	var msg domain.Message
	if numChanges > 0 {
		msg = domain.KnowledgeUpdated{
			NumChanges: numChanges,
		}
		return msg, nil
	}
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

func (c *Campaign) onListenerStopped(ctx context.Context, event domain.Event) (domain.Message, error) {
	ev := event.(c2.ListenerStopped)
	id := fmt.Sprintf("%s_%d", ev.Name, ev.Port)

	_, ok := c.listeners[id]
	delete(c.listeners, id)
	if !ok {
		slog.Error(fmt.Sprintf("Can't stop unknown listener '%s'", ev.Name))

	}

	return nil, nil
}

func StartCampaign(mb bus.MessageBus) *Campaign {
	campaign := &Campaign{
		sessions:   make(map[string]c2.Session),
		entities:   make(map[string]domain.Entity),
		relations:  make([]domain.Relation, 0),
		listeners:  make(map[string]domain.Listener),
		identities: make(map[string]domain.Identity),
	}
	mb.Subscribe(c2.ListenerReady{}, campaign.onListenerReady)
	mb.Subscribe(c2.ListenerStopped{}, campaign.onListenerStopped)
	mb.Subscribe(c2.SessionStarted{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		return campaign.onNewSession(event.(c2.SessionStarted))
	})
	mb.Subscribe(c2.SessionClosed{}, func(ctx context.Context, event domain.Event) (domain.Message, error) {
		return campaign.onSessionClosed(event.(c2.SessionClosed))
	})
	mb.Subscribe(domain.NewFacts{}, campaign.onNewFacts)
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
