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

	var system domain.Entity

	// see if a pod with that name is already known, if so update it or add a new 'system'
	for _, e := range c.kb.GetEntities() {
		if strings.HasSuffix(e.GetId(), "pod/"+ev.Session.Hostname) {
			if pod, ok := e.(domain.K8sEntity); ok {
				if ev.Session.IsRoot {
					pod.AccessLevel = domain.RootExec
				} else {
					pod.AccessLevel = domain.UserExec
				}
				system = pod
			} else {
				slog.Warn("onNewSession: Dont know how to update accesslevel of " + e.GetId())
				system = e
			}
			break
		}
	}

	if system == nil {
		accessLevel := domain.UserExec
		if ev.Session.IsRoot {
			accessLevel = domain.RootExec
		}

		system = domain.System{
			Name:        ev.Session.Hostname,
			OS:          ev.Session.Os,
			AccessLevel: accessLevel,
		}
	}

	msg := domain.NewFacts{
		Entities:  []domain.Entity{system},
		Relations: []domain.Relation{},
	}
	return msg, nil
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
	kb             KnowledgeBase
	activeIdentity string
	listeners      map[string]domain.Listener
	sessions       map[string]c2.Session
	identities     map[string]domain.Identity
}

func (c *Campaign) GetEntities() map[string]domain.Entity {
	return c.kb.GetEntities()
}

func (c *Campaign) GetEntityById(id string) (domain.Entity, bool) {
	e, ok := c.kb.GetEntity(id)
	return e, ok
}

func (c *Campaign) GetEntityByName(name, ns string) (domain.Entity, bool) {
	for _, e := range c.kb.GetEntities() {
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

func (c *Campaign) AddEntities(entities ...domain.Entity) int {
	numChanges, err := c.kb.AddEntities(entities...)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to insert %d entities: %v", len(entities), err))
	}
	return numChanges
}

func (c *Campaign) onNewFacts(ctx context.Context, msg domain.Message) (domain.Message, error) {
	// TODO: properly track how many changes the update contained
	numChanges := 0
	ev := msg.(domain.NewFacts)
	numChanges += c.AddEntities(ev.Entities...)

	for _, identity := range ev.Identities {
		// if there is no active identity, use the first encountered Id as the active oneo
		if c.activeIdentity == "" {
			c.activeIdentity = identity.Name
		}
		c.identities[identity.Name] = identity
	}

	// TODO: reconcile new entities with existing ones
	var response domain.Message
	if numChanges > 0 {
		response = domain.KnowledgeUpdated{
			NumChanges: numChanges,
		}
		return response, nil
	}
	return nil, nil
}

func (c *Campaign) onListenerReady(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(c2.ListenerReady)
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

func (c *Campaign) onListenerStopped(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(c2.ListenerStopped)
	id := fmt.Sprintf("%s_%d", ev.Name, ev.Port)

	_, ok := c.listeners[id]
	delete(c.listeners, id)
	if !ok {
		slog.Error(fmt.Sprintf("Can't stop unknown listener '%s'", ev.Name))

	}

	return nil, nil
}

func NewCampaign() *Campaign {
	kg := InitGraph()

	return &Campaign{
		kb:         kg,
		sessions:   make(map[string]c2.Session),
		listeners:  make(map[string]domain.Listener),
		identities: make(map[string]domain.Identity),
	}

}

func StartCampaign(mb bus.MessageBus) *Campaign {
	campaign := NewCampaign()
	mb.Subscribe(c2.ListenerReady{}, campaign.onListenerReady)
	mb.Subscribe(c2.ListenerStopped{}, campaign.onListenerStopped)
	mb.Subscribe(c2.SessionStarted{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return campaign.onNewSession(msg.(c2.SessionStarted))
	})
	mb.Subscribe(c2.SessionClosed{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return campaign.onSessionClosed(msg.(c2.SessionClosed))
	})
	mb.Subscribe(domain.NewFacts{}, campaign.onNewFacts)
	mb.Subscribe(domain.EnvVarsExtracted{}, campaign.onEnvVarsExtracted)

	err := mb.Publish(CampaignStarted{})
	if err != nil {
		panic(err)
	}
	return campaign
}

func (c Campaign) GroundAction(action domain.Message, targetId string) (domain.Message, error) {
	execCmd, ok := action.(domain.ExecTTP)
	if !ok {
		return action, fmt.Errorf("expected action to be of type domain.ExecTTP")
	}

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

		execCmd.Cmd = template
	}

	var target domain.Entity
	// check if it is targeted
	if t, ok := action.(domain.Targeter); ok {
		target, ok = c.kb.GetEntity(targetId)
		if ok {
			execCmd.Target = t.SetTarget(target)
		}
	}

	// if it need userRead/ userExecute, identify the necessary channel
	if execCmd.TTP.Requires.AccessLevel != domain.NoAccess {
		if system, ok := target.(domain.K8sEntity); ok {
			if system.AccessLevel.Satisfies(execCmd.TTP.Requires.AccessLevel) {
				execCmd.C2Channel = nil
			}
		}
	}

	return execCmd, nil
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

func (c *Campaign) onEnvVarsExtracted(ctx context.Context, msg domain.Message) (domain.Message, error) {
	return analyzeEnvironmentVariables(msg.(domain.EnvVarsExtracted))
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
