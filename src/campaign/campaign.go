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

type Campaign struct {
	kb             KnowledgeBase
	activeIdentity string
	listeners      map[string]domain.Listener
	sessions       map[string]c2.Session
	identities     map[string]domain.Identity
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

// type CampaignStarted struct {
// }

// func (c CampaignStarted) String() string {
// 	return "campaign started"
// }

func StartCampaign(mb bus.MessageBus) *Campaign {
	campaign := NewCampaign()
	mb.Subscribe(domain.C2Connected{}, campaign.onC2Connected)
	mb.Subscribe(c2.ListenerReady{}, campaign.onListenerReady)
	mb.Subscribe(c2.ListenerStopped{}, campaign.onListenerStopped)
	mb.Subscribe(c2.SessionStarted{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return campaign.onNewSession(msg.(c2.SessionStarted))
	})
	mb.Subscribe(c2.SessionClosed{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return campaign.onSessionClosed(msg.(c2.SessionClosed))
	})
	mb.Subscribe(domain.NewFacts{}, campaign.onNewFacts)
	mb.Subscribe(domain.ServiceAccountTokenExtracted{}, campaign.onServiceAccountTokenExtracted)
	mb.Subscribe(domain.PrintGraph{}, campaign.onPrintGraph)
	mb.Subscribe(domain.EnvVarsExtracted{}, campaign.onEnvVarsExtracted)

	// err := mb.Publish(CampaignStarted{})
	// if err != nil {
	// 	panic(err)
	// }
	return campaign
}

func (c *Campaign) GetEntities() map[string]domain.Entity {
	return c.kb.GetEntities()
}

func (c *Campaign) GetPods() []domain.Pod {
	pods := make([]domain.Pod, 0)
	for _, e := range c.kb.GetEntities() {
		if p, ok := e.(domain.Pod); ok {
			pods = append(pods, p)
		}
	}
	return pods
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

func (c *Campaign) AddRelations(relations ...domain.Relation) int {
	numChanges, err := c.kb.AddRelations(relations...)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to insert %d relations: %v", len(relations), err))
	}
	return numChanges
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
		// TODO: Entity can't be converted to K8sEntity -> use reflection?
		if system, ok := target.(domain.K8sEntity); ok {
			if system.AccessLevel.Satisfies(execCmd.TTP.Requires.AccessLevel) {
				c2Channel, err := findC2Channel(c.kb, target)
				if err == nil {
					execCmd.C2Channel = c2Channel
				}

			}
		}
	}

	return execCmd, nil
}

func (c Campaign) GetC2s() []domain.C2System {
	c2s := make([]domain.C2System, 0)
	for _, entity := range c.GetEntities() {
		if c2, ok := entity.(domain.C2System); ok {
			c2s = append(c2s, c2)
		}
	}
	return c2s
}

func findC2Channel(kg KnowledgeBase, target domain.Entity) (domain.C2Channel, error) {
	for _, entity := range kg.GetEntities() {
		if c2, ok := entity.(domain.C2System); ok {
			_, relations, err := kg.GetPath(c2.GetId(), target.GetId())
			if err != nil {
				slog.Warn(fmt.Sprintf("Failed to get path from '%s' to '%s'", c2.GetId(), target.GetId()))
			}

			if l := len(relations); l > 0 {
				rel := relations[0]
				if l > 1 {
					slog.Info(fmt.Sprintf("Got %d possible channels, using 1st one", l))
				}
				return rel.(domain.C2Channel), nil
			}
		}
	}

	return nil, fmt.Errorf("No channel found")
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

func (c Campaign) syncCapabilities() error {
	// evaluate potential relationships based on RBAC permissions
	accessRelations := make([]domain.Relation, 0)
	c2s := c.GetC2s()
	// TODO: Fix this: pods are not properly retrieved?! ... fking Go ...
	pods := c.GetPods()

	for _, identity := range c.identities {
		if identity.Can("pod/exec") {
			for _, p := range pods {
				for _, c2 := range c2s {
					accessRelations = append(accessRelations, domain.CanAccess{
						SourceId:    c2.GetId(),
						TargetId:    p.GetId(),
						Identity:    identity,
						AccessLevel: domain.UserExec,
					})
				}
				p.AccessLevel = domain.UserExec
				_ = c.kb.AddEntity(p) // update the entity
			}
		}
	}
	c.AddRelations(accessRelations...)

	return nil
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
