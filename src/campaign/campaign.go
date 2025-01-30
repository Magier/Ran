package campaign

import (
	"context"
	"fmt"
	"log/slog"
	"strings"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
)

type Campaign struct {
	kb             KnowledgeBase
	activeIdentity string
	armory         armory.Armory
	listeners      map[string]domain.Listener
	sessions       map[string]domain.Session
	identities     map[string]domain.Identity
}

func NewCampaign(armory armory.Armory) *Campaign {
	kg := InitGraph()

	return &Campaign{
		kb:         kg,
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

func (c Campaign) GroundAction(ttp domain.TTP, targetId string) (domain.Message, error) {
	execCmd := domain.ExecTTP{
		CommandImpl: domain.NewCmd(),
		TTP:         ttp,
	}

	execCmd.CmdVariants = groundTTP(ttp, func(template string) (string, error) {
		return c.groundCmdTemplate(template, ttp.Args)
	})

	var target domain.Entity
	target, ok := c.kb.GetEntity(targetId)
	if ok {
		execCmd.Target = target
	}

	if isActionOnRemoteTarget(execCmd.TTP) {
		var system domain.Pod
		switch t := target.(type) {
		case domain.Pod:
			system = t
		case domain.ServiceAccount:
			var err error
			var cmdVariants []domain.CmdVariant

			if system, cmdVariants, err = c.groundTtpOnServiceAccount(execCmd.TTP, t); err == nil {
				execCmd.CmdVariants = append(execCmd.CmdVariants, cmdVariants...)
			} else {
			}
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

func groundTTP(ttp domain.TTP, fn GroundFn) []domain.CmdVariant {
	// if a specific Cmd is set, treat it as the top priority variant
	cmdVariants := make([]domain.CmdVariant, 0)
	// TODO order the variants depending on utility/priority
	for _, cmdVariant := range ttp.CmdVariants {
		if cmd, err := fn(cmdVariant.Command); err == nil {
			cmdVariants = append(cmdVariants, domain.CmdVariant{Key: cmdVariant.Key, Command: cmd})
		} else {
			slog.Error(err.Error())
		}
	}
	return cmdVariants
}

func (c Campaign) groundCmdTemplate(template string, variables map[string]string) (string, error) {
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

	return template, nil
}

func (c Campaign) groundTtpOnServiceAccount(ttp domain.TTP, t domain.ServiceAccount) (domain.Pod, []domain.CmdVariant, error) {
	var system domain.Pod
	// *vomit*
	if owner, ok := t.GetOwner(); ok {
		if e, ok := c.GetEntityByName(owner.Name, t.Namespace); ok {
			if pod, ok := e.(domain.Pod); ok {
				system = pod
			}
		}
	} else if users, err := c.kb.GetIncomingEntities(t, domain.Uses{}); err == nil {
		if len(users) > 0 {
			user := users[0]
			if pod, ok := user.(domain.Pod); ok {
				system = pod
			}
		}
	}

	cmdVariants := groundTTP(ttp, func(template string) (string, error) {
		return c.groundServiceAccountTemplate(template, t)
	})

	// TODO: properly supply values of the SA token to the TTP
	return system, cmdVariants, nil

}

func (c Campaign) groundServiceAccountTemplate(template string, sa domain.ServiceAccount) (string, error) {
	if strings.Contains(template, "${API_SERVER}") {
		apiUrl, err := c.GetApiUrl(true)
		if err != nil {
			slog.Error("Ground SA Template", "", err.Error())
		} else {
			template = strings.Replace(template, "${API_SERVER}", apiUrl, -1)
		}
	}
	template = strings.Replace(template, "${TOKEN}", sa.Token.Raw, -1)
	template = strings.Replace(template, "${CA_PATH}", "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt", -1)
	template = strings.Replace(template, "${NS}", sa.Namespace, -1)
	template = strings.Replace(template, "${SA_NAME}", sa.Name, -1)

	template = strings.ReplaceAll(template, "\n", " ")
	template = strings.ReplaceAll(template, "\t", " ")

	return template, nil
}

// Determine if the TTP will be executed in the target environment, or the operator infrastructure
func isActionOnRemoteTarget(ttp domain.TTP) bool {
	tactics := ttp.Tactics
	numTactics := len(tactics)
	if numTactics == 0 {
		return false
	} else if numTactics > 1 {
		slog.Debug(fmt.Sprintf("TTP %s has %d tactics; using the first to determine its nature", ttp.Name, numTactics))
	}
	switch tactics[0] {
	case domain.Reconnaissance, domain.ResourceDevelopment:
		return false
	default:
		return true
	}
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

func (c Campaign) GetC2(name string) (domain.C2System, bool) {
	for _, entity := range c.GetEntities() {
		if c2, ok := entity.(domain.C2System); ok {
			if c2.Name == name {
				return c2, true
			}
		}
	}
	return domain.C2System{}, false
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
