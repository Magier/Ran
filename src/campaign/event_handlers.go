package campaign

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"os"
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
	"github.com/Magier/Ran/parsers"
	"github.com/dominikbraun/graph/draw"
	"github.com/goccy/go-graphviz"
)

func (c *Campaign) onActionSelected(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.ActionSelected)

	ttp, ok := c.armory.GetTTP(ev.ActionID)
	if !ok {
		msg := fmt.Sprintf("No TTP with ID '%s' found!", ev.ActionID)
		slog.Error(msg)
		return nil, errors.New(msg)
	}

	msg, err := c.GroundAction(ttp, ev.TargetID, ev.Args)
	if err != nil {
		slog.Error(fmt.Sprintf("Could not ground action: %v\n", err))
	}
	return msg, err
}

func (c *Campaign) onExecuteTTP(ctx context.Context, msg domain.Message) (domain.Message, error) {
	cmd := msg.(domain.ExecTTP)
	err := c.trail.AddNewStep(cmd)
	return nil, err
}

func (c *Campaign) onTTPExecuted(ctx context.Context, msg domain.Message) (domain.Message, error) {
	cmd := msg.(domain.TTPExecuted)
	ttp := cmd.TTP

	var results []string = cmd.Results
	// if cmd.Success {
	// 	for _, r := range cmd.Results {
	// 		results = append(results, s)
	// 	}
	// } else {
	// 	results = []string{cmd.Reason}
	// }

	c.trail.CompleteStep(cmd.ID, cmd.TTP, cmd.Success, results)

	newFacts := NewFacts{}
	removedFacts := RemovedFacts{}

	// TODO: Temporary workaround: dedicated handling for newly created pods
	// ensure the podCfg is properly provided regardless of which command variant is executed
	for _, technique := range cmd.TTP.Techniques {
		if technique == "T1610" || strings.ToLower(technique) == "deploy container" {
			new, removed, err := analyzeDeployPodResult(cmd)

			if err != nil {
				slog.Error(fmt.Sprintf("Failed to analyze deploy pod result: %v", err))
			}

			// newFacts.Update(new)
			msg, err := c.UpdateFacts(new, removed)
			if err != nil {
				slog.Error("[onTTPExecuted] Couldn't update facts when adding new container: " + err.Error())
			} else {
				// TODO: fix this dirty hack to return multiple messages from a single function
				if err := c.bus.Publish(msg); err != nil {
					slog.Error("[onTTPExecuted] failed to publish updateFacts event: " + err.Error())
				}
			}
		}
	}

	// TODO: properly streamline the various ways to handle the results of a TTP execution
	// post processing will yield the final message
	if fn := parsers.GetParser(ttp.Parser); fn != nil {
		event, err := fn(cmd, cmd.Target, cmd.Results...)
		return event, err
	}

	if len(ttp.Effects) > 1 {
		slog.Info(fmt.Sprintf("TTP has %d effects; using only first one", len(ttp.Effects)))
	}
	for _, effect := range ttp.Effects {
		new, removed := parseEffect(effect, cmd.Target, cmd.Results...)
		newFacts.Update(new)
		removedFacts.Update(removed)
	}

	return c.UpdateFacts(newFacts, removedFacts)
}

func (c *Campaign) onC2Connected(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.C2Connected)

	// builtin C2 is part of Ran C2
	if ev.Name == c2.BuiltInC2 {
		return nil, nil
	}

	system := domain.C2System{
		Kind: ev.Kind,
		Name: ev.Name,
		IPs:  []net.IP{ev.IP},
	}

	rels := []domain.Relation{}

	c2s := c.GetC2s()
	ran := c2s[0] // Ran is always the first C2
	operatesRel := domain.Operates{Operator: ran, System: system}
	c.AddRelations(operatesRel)
	rels = append(rels, operatesRel)

	return c.UpdateFacts(NewFacts{Entities: []domain.Entity{system}, Relations: rels}, RemovedFacts{})
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
	err := c.syncCapabilities()
	if err != nil {
		slog.Error(err.Error())
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

	// TODO: analyze session:
	// ideas:
	// hostname != pod-name -> maybe hostPID/hostIPC etc. flags are set on pod?
	// or maybe the system is a node

	// convert the communication channel to a relationship
	c2Channel := domain.ImplantC2Channel{
		SessionId: ev.Session.Id,
		SourceId:  fmt.Sprintf("%s/%s", "c2", ev.C2Kind),
		Kind:      ev.C2Kind,
		Target:    system,
		// Protocol  string
	}

	hasSession := domain.HasC2Session{
		System:  system,
		Session: ev.Session,
	}

	return c.UpdateFacts(
		NewFacts{[]domain.Entity{system, ev.Session}, []domain.Relation{c2Channel, hasSession}, nil, nil},
		RemovedFacts{},
	)
}

func (c *Campaign) onSessionClosed(ev c2.SessionClosed) (domain.Message, error) {
	_, ok := c.sessions[ev.Session.Id]
	if !ok {
		return nil, fmt.Errorf("Unknwon session '%s' could not be closed", ev.Session.Id)
	}
	delete(c.sessions, ev.Session.Id)

	// TODO: remove session from all relations
	slog.Error(fmt.Sprintf("Session removal for '%s' not yet implemented", ev.Session.Id))
	return nil, nil
}

func (c *Campaign) onEnvVarsExtracted(ctx context.Context, msg domain.Message) (domain.Message, error) {
	newFacts, removedFacts, err := analyzeEnvironmentVariables(msg.(domain.EnvVarsExtracted))
	if err != nil {
		return nil, err
	} else {
		return c.UpdateFacts(newFacts, removedFacts)
	}
}
func (c *Campaign) onServiceAccountTokenExtracted(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.ServiceAccountTokenExtracted)
	newFacts, removedFacts, err := analyzeServiceAccountToken(ev.Token)
	if err != nil {
		return nil, err
	} else {
		return c.UpdateFacts(newFacts, removedFacts)
	}
}

func (c *Campaign) onTokenPermissionsExtracted(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.TokenPermissionsRetrieved)
	sa := ev.ServiceAccount

	for _, rule := range ev.ResourceRules {
		sa.Can = append(sa.Can, domain.RbacPermission{
			Verbs:         rule.Verbs,
			ResourceTypes: rule.Resources,
			ResourceNames: rule.ResourceNames,
			ApiGroups:     rule.APIGroups,
			Scope:         sa.GetNamespace(),
		})
	}

	for _, rule := range ev.ResourceRules {
		sa.Can = append(sa.Can, domain.RbacPermission{
			Verbs:         rule.Verbs,
			ResourceTypes: rule.Resources,
			ResourceNames: rule.ResourceNames,
			ApiGroups:     rule.APIGroups,
			Scope:         "*",
		})
	}

	return c.UpdateFacts(NewFacts{Entities: []domain.Entity{sa}}, RemovedFacts{})
}

func (c *Campaign) onListenerReady(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(c2.ListenerReady)
	listenerID := ev.Name

	c.trail.CompleteStep(ev.CmdId, domain.TTP{}, true, []string{fmt.Sprintf("Listener on C2 '%s' port %d ready", ev.C2Server, ev.Port)})
	c2, ok := c.GetC2(ev.C2Server)
	if !ok {
		return nil, fmt.Errorf("No C2 '%s' found", ev.C2Server)
	}

	var c2IP net.IP
	if len(c2.IPs) > 0 {
		c2IP = c2.IPs[0]
	}

	listener := domain.Listener{
		ID:         listenerID,
		IP:         c2IP,
		Port:       ev.Port,
		Protocol:   ev.Protocol,
		Redirector: "",
	}
	c.listeners[listenerID] = listener

	return c.UpdateFacts(
		NewFacts{
			Entities: []domain.Entity{listener},
			Relations: []domain.Relation{
				domain.ListenesOn{
					C2ID:       c2.GetId(),
					ListenerID: listenerID,
					Port:       int(ev.Port),
				},
			}},
		RemovedFacts{},
	)
}

func (c *Campaign) onListenerStopped(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(c2.ListenerStopped)
	id := fmt.Sprintf("%s_%d", ev.Name, ev.Port)

	_, ok := c.listeners[id]
	delete(c.listeners, id)
	if !ok {
		slog.Error(fmt.Sprintf("Can't stop unknown listener '%s'", ev.Name))

	}
	// TODO: remove listener from all relations
	slog.Error(fmt.Sprintf("Listener removal for '%s' not yet implemented", ev.Name))
	return nil, nil
}

func (c *Campaign) onNewK8sResourceCreated(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.NewK8sResourceCreated)
	slog.Info(fmt.Sprintf("New K8s resource created: %s", ev.Resource))

	relations := make([]domain.Relation, 0)
	if ev.CreatorID != "" {
		if creator, ok := c.GetEntityById(ev.CreatorID); ok {
			relations = append(relations, domain.Created{
				Creator: creator,
				Object:  ev.Resource,
			})
		}
	}

	// TODO: handle this properly depending on the effects case where a rolebinding is created
	if binding, ok := ev.Resource.(domain.RoleBinding); ok {
		roleEntity, hasRole := c.GetEntityById(binding.RoleID)
		role, isRole := roleEntity.(domain.Role)
		if hasRole && isRole {
			for _, subjectID := range binding.SubjectIDs {
				subject, hasSubject := c.GetEntityById(subjectID)
				sa, isSa := subject.(domain.ServiceAccount)

				if hasSubject && isSa {
					relations = append(relations, domain.BindsRole{
						Role:    role,
						Subject: sa,
					})
				}
			}
		} else {
			return nil, fmt.Errorf("RoleBinding '%s' references unknown role '%s'", binding.GetId(), binding.RoleID)
		}
	}

	return c.UpdateFacts(
		NewFacts{
			Entities:  []domain.Entity{ev.Resource},
			Relations: relations,
		}, RemovedFacts{},
	)
}

func parseEffect(effect string, source domain.Entity, args ...string) (NewFacts, RemovedFacts) {
	if len(args) == 0 {
		slog.Warn("Can't parse effect %s because there are no arguments")
		return NewFacts{}, RemovedFacts{}
	}

	entities := []domain.Entity{}
	switch strings.ToLower(effect) {
	// TODO: set these 'attribute' effects via reflection
	case "target.ip":
		if pod, ok := source.(domain.Pod); ok {
			ips := []net.IPAddr{}
			res := args[0]
			for _, ip := range strings.Split(res, " ") {
				parsedIP := net.ParseIP(ip)
				if parsedIP == nil {
					slog.Error("Failed to parse IP")
					break
				}
				ips = append(ips, net.IPAddr{IP: parsedIP})
			}
			pod.IPs = ips
			entities = append(entities, pod)
		}
	case "k8s.podlist":
		res := args[0]
		list, err := k8s.ParsePodList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse PodList: %v", err))
		} else {
			for _, res := range list.Items {
				entities = append(entities, domain.NewPodFromK8sSpec(res))
			}
		}
	case "k8s.serviceaccountlist":
		res := args[0]
		list, err := k8s.ParseServiceAccountList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse PodList: %v", err))
		} else {
			for _, res := range list.Items {
				entities = append(entities, domain.NewServiceAccountFromK8sSpec(res))
			}
		}
	}
	return NewFacts{Entities: entities}, RemovedFacts{}
}

func (c *Campaign) onPrintGraph(ctx context.Context, msg domain.Message) (domain.Message, error) {
	if kb, ok := c.kb.(BuiltInKnowledgeBase); ok {
		var buf bytes.Buffer
		err := draw.DOT(kb.graph, &buf)
		if err != nil {
			return nil, err
		}

		// taken from https://github.com/goccy/go-graphviz?tab=readme-ov-file#3-render-graph
		g, err := graphviz.New(ctx)
		if err != nil {
			return nil, err
		}
		graph, err := graphviz.ParseBytes(buf.Bytes())
		if err != nil {
			return nil, err
		}
		path := "topo.png"
		if err := g.RenderFilename(ctx, graph, graphviz.PNG, path); err != nil {
			return nil, err
		}
		return domain.GraphRendered{Path: path}, nil
	}
	return nil, nil
}

func (c *Campaign) onSaveAttackFlow(ctx context.Context, msg domain.Message) (domain.Message, error) {
	cmd, ok := msg.(domain.SaveAttackFlow)
	if !ok {
		return nil, errors.New("Received invalid valiad SaveAttackFlow command")
	}

	af, err := c.trail.ConvertToAttackFlow()
	if err != nil {
		return nil, errors.New("Received invalid valiad SaveAttackFlow command")
	}
	data, err := af.Marshal()
	if err != nil {
		return nil, errors.New("Couldn't marshal attack flow to JSON: " + err.Error())
	}
	err = os.WriteFile(cmd.Path, []byte(data), 0644)
	if err != nil {
		return nil, fmt.Errorf("Failed o save attack flow: %w", err)
	}
	return domain.AttackFlowSaved{Path: cmd.Path}, nil
}
