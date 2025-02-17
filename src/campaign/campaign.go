package campaign

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strings"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
	"github.com/google/uuid"
)

type Campaign struct {
	kb             KnowledgeBase
	trail          AuditTrail
	activeIdentity string
	armory         armory.Armory
	listeners      map[string]domain.Listener
	sessions       map[string]domain.Session
	identities     map[string]domain.Identity
}

func NewCampaign(armory armory.Armory) *Campaign {
	kg := InitGraph()

	_ = kg.AddEntity(domain.C2System{Name: "Ran", Kind: "Ran"})

	return &Campaign{
		kb:         kg,
		trail:      NewAuditTrail(),
		armory:     armory,
		sessions:   make(map[string]domain.Session),
		listeners:  make(map[string]domain.Listener),
		identities: make(map[string]domain.Identity),
	}
}

// type CampaignStarted struct {
// }

// func (c CampaignStarted) String() string {
// 	return "campaign started"
// }

func StartCampaign(mb bus.MessageBus, armory armory.Armory) *Campaign {
	campaign := NewCampaign(armory)
	mb.Subscribe(domain.C2Connected{}, campaign.onC2Connected)
	mb.Subscribe(domain.ExecTTP{}, campaign.onExecuteTTP)
	mb.Subscribe(domain.TTPExecuted{}, campaign.onTTPExecuted)
	mb.Subscribe(domain.TTPFailed{}, campaign.onTTPFailed)
	mb.Subscribe(c2.ListenerReady{}, campaign.onListenerReady)
	mb.Subscribe(c2.ListenerStopped{}, campaign.onListenerStopped)
	mb.Subscribe(c2.SessionStarted{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return campaign.onNewSession(msg.(c2.SessionStarted))
	})
	mb.Subscribe(c2.SessionClosed{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return campaign.onSessionClosed(msg.(c2.SessionClosed))
	})
	mb.Subscribe(domain.ActionSelected{}, campaign.onActionSelected)
	mb.Subscribe(domain.FactsChanged{}, campaign.onFactsChanged)
	mb.Subscribe(domain.ServiceAccountTokenExtracted{}, campaign.onServiceAccountTokenExtracted)
	mb.Subscribe(domain.TokenPermissionsRetrieved{}, campaign.onTokenPermissionsExtracted)
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

func (c *Campaign) GetApiUrl(internalIp bool) (string, error) {
	if internalIp {
		// TODO use actual IP/port
		slog.Warn("Using hardcoded internal API IP")
		return "https://10.96.0.1", nil
		// return "https://kubernetes.default.svc.cluster.local", nil
	}
	return "", fmt.Errorf("%t K8s API URL unknown", internalIp)
}

func (c *Campaign) GetIdentities() map[string]domain.Identity {
	return c.identities
}

func (c *Campaign) GetSessions() []domain.Session {
	sessions := make([]domain.Session, 0, len(c.sessions))
	for _, s := range c.sessions {
		sessions = append(sessions, s)
	}
	return sessions
}

func (c *Campaign) GetGraph() AdjacencyList {
	return c.kb.GetAdjecencyList()
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

func (c Campaign) selectBestCommandVariant(ttp domain.TTP) (domain.CmdVariant, error) {
	// TODO: select the variant to execute
	// - keep track of tried variants
	// - favor robust C2 over builtin one

	if len(ttp.CmdVariants) == 0 {
		return domain.CmdVariant{}, errors.New("No valid Command Variant available for TTP " + ttp.GetID())
	}
	return ttp.CmdVariants[0], nil
}

func (c Campaign) GroundAction(ttp domain.TTP, targetId string, args map[string]string) (domain.Message, error) {
	execCmd := domain.ExecTTP{
		CommandImpl: domain.NewCmd(),
		TTP:         ttp,
		Args:        args,
	}
	execCmd.SetID(uuid.New().String())

	// it's a technique on the C2 side to prepare the infrastructure, not in the target environment
	cmdMsg, err := hydrateCommand(ttp, execCmd.ID, args)
	if err != nil {
		slog.Warn(err.Error())
	}
	execCmd.CommandMsg = cmdMsg

	execCmd.Variant, err = c.selectBestCommandVariant(ttp)
	// TODO: re-enable this error check
	// if err != nil {
	// 	return nil, err
	// }

	execCmd.Variant.Command = c.groundCmdTemplate(execCmd.Variant.Command, args)

	var target domain.Entity
	target, ok := c.kb.GetEntity(targetId)
	if ok {
		execCmd.Target = target
	}

	if isActionOnRemoteTarget(execCmd.TTP, execCmd.Variant) {
		var system domain.Pod
		switch t := target.(type) {
		case domain.Pod:
			system = t
		case domain.ServiceAccount:
			system = c.getServiceAccountOwner(t)
			execCmd.Variant.Command = c.groundServiceAccountTemplate(execCmd.Variant.Command, t)
		}

		if system.AccessLevel.Satisfies(execCmd.TTP.Requires.AccessLevel) {
			c2Channel, err := findC2Channel(c.kb, target)
			if err == nil {
				execCmd.C2Channel = c2Channel
			}
		}
	}

	return execCmd, nil
}

type GroundFn func(string) (string, error)

func (c Campaign) groundCmdTemplate(template string, variables map[string]string) string {
	if strings.Contains(template, "${API_SERVER}") {
		apiUrl, err := c.GetApiUrl(true)
		if err != nil {
			slog.Error("Ground Template", "", err.Error())
		} else if apiUrl == "" {
			slog.Info("No API Server URL found when grounding command")
		} else {
			template = strings.Replace(template, "${API_SERVER}", apiUrl, -1)
		}
	}
	if strings.Contains(template, "${LISTENER}") {
		listener, ok := c.GetListener(domain.TCP)
		if ok {
			template = inflateListenerTemplate(listener, template)
		} else {
			slog.Info("No suitable listener found!")
		}
	}
	if strings.Contains(template, "${FILESHARE_PORT}") {
		filesharePort, ok := c.GetFileshare()
		if ok {
			p := fmt.Sprint(filesharePort)
			template = strings.Replace(template, "${FILESHARE_PORT}", p, -1)
		} else {
			slog.Info("No suitable fileshare found!")
		}
	}

	for key, v := range variables {
		templateVariable := fmt.Sprintf("${%s}", strings.ToUpper(key))
		template = strings.Replace(template, templateVariable, v, -1)
	}

	return template
}

func (c Campaign) getServiceAccountOwner(sa domain.ServiceAccount) domain.Pod {
	var system domain.Pod
	// *vomit*
	if owner, ok := sa.GetOwner(); ok {
		if e, ok := c.GetEntityByName(owner.Name, sa.Namespace); ok {
			if pod, ok := e.(domain.Pod); ok {
				system = pod
			}
		}
	} else if users, err := c.kb.GetIncomingEntities(sa, domain.Uses{}); err == nil {
		if len(users) > 0 {
			user := users[0]
			if pod, ok := user.(domain.Pod); ok {
				system = pod
			}
		}
	}
	return system
}

func (c Campaign) groundServiceAccountTemplate(template string, sa domain.ServiceAccount) string {
	template = strings.Replace(template, "${TOKEN}", sa.Token.Raw, -1)
	template = strings.Replace(template, "${CA_PATH}", "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt", -1)
	template = strings.Replace(template, "${NS}", sa.Namespace, -1)
	template = strings.Replace(template, "${SA_NAME}", sa.Name, -1)

	return template
}

// Determine if the TTP will be executed in the target environment, or the operator infrastructure
func isActionOnRemoteTarget(ttp domain.TTP, cmd domain.CmdVariant) bool {
	if cmd.IsLocalCommand {
		return false
	}

	// TODO: get rid of this approach
	switch ttp.Tactic {
	case domain.Reconnaissance, domain.ResourceDevelopment:
		return false
	default:
		return true
	}
}

func (c Campaign) GetC2s() []domain.C2System {
	return c.kb.GetC2s()
}

func (c Campaign) GetC2(name string) (domain.C2System, bool) {
	for _, c2 := range c.GetC2s() {
		if c2.Name == name {
			return c2, true
		}
	}
	return domain.C2System{}, false
}

func findC2Channel(kg KnowledgeBase, target domain.Entity) (domain.C2Channel, error) {
	for _, c2 := range kg.GetC2s() {
		_, relations, err := kg.GetPath(c2.GetId(), target.GetId())
		if err != nil {
			if !strings.HasPrefix(err.Error(), "target vertex not reachable") {
				slog.Debug(fmt.Sprintf("Failed to get path from '%s' to '%s'", c2.GetId(), target.GetId()))
			}
			continue
		}

		if l := len(relations); l > 0 {
			rel := relations[0]
			if l > 1 {
				slog.Info(fmt.Sprintf("Got %d possible channels, using 1st one", l))
			}

			if ch, ok := rel.(domain.C2Channel); ok {
				return ch, nil
			} else if canAccess, ok := rel.(domain.CanAccess); ok {
				if target, ok := kg.GetEntity(canAccess.TargetId); ok {
					return domain.PodExecC2Channel{
						SourceId: canAccess.SourceId,
						Target:   target,
						Identity: canAccess.Identity,
					}, nil
				} else {
					return nil, fmt.Errorf("Could not identify target %s", canAccess.TargetId)
				}
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
	// c2s := c.GetC2s()
	c2, ok := c.GetC2("Ran")
	if !ok {
		return errors.New("Couldn't retrieve Ran from KG to sync capabilities")
	}
	pods := c.GetPods()

	for _, identity := range c.identities {
		if identity.Can("pod/exec") {
			for _, p := range pods {
				accessRelations = append(accessRelations, domain.CanAccess{
					SourceId:    c2.GetId(),
					TargetId:    p.GetId(),
					Identity:    identity,
					AccessLevel: domain.UserExec,
				})
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
	if strings.Contains(template, "${LISTENER_PORT}") {
		p := fmt.Sprint(listener.Port)
		template = strings.Replace(template, "${LISTENER_PORT}", p, -1)
	}

	var dst string
	if listener.Redirector != "" {
		dst = listener.Redirector
	} else {
		dst = listener.IP.String()
	}

	template = strings.Replace(template, "${LISTENER}", dst, -1)
	return template
}
