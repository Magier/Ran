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
	k8s_types "github.com/Magier/Ran/k8sclient/types"
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

	if ttp.Tactic == "InitialAccess" && ttp.ID == "use-kubeconfig" {
		ns := ev.Args["Namespace"]
		name := ev.Args["TargetName"]

		if strings.HasPrefix(name, "ns/") {
			var err error
			ns, _, name, err = UnpackResourceID(name)
			if err != nil {
				slog.Error(fmt.Sprintf("Failed to unpack resource ID '%s': %v", name, err))
				return nil, err
			}
		}

		return c.SetTarget(ns, name)
	}

	msg, err := c.GroundAction(ttp, ev.TargetID, ev.ProcedureID, ev.Args)
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

func (c *Campaign) onC2TTPExecuted(ctx context.Context, msg domain.Message) (domain.Message, error) {
	var err error
	c2Ev := msg.(c2.TTPExecuted)
	results := c2Ev.Results

	var ev domain.TTPExecuted
	step, ok := c.trail.GetOpenStep(c2Ev.ID)
	if !ok {
		slog.Warn(fmt.Sprintf("Received TTPExecuted for unknown step ID '%s'", c2Ev.ID))
	} else {
		ev = domain.NewTTPExecutedWithResult(step.ExecCommand, c2Ev.Success, c2Ev.Results, c2Ev.ExecutedOn)
	}

	factsUpdate := factsUpdate{}

	// TODO: Temporary workaround: dedicated handling for newly created pods
	// ensure the podCfg is properly provided regardless of which procedure is executed
	for _, technique := range ev.TTP.Techniques {
		if technique == "T1610" || strings.ToLower(technique) == "deploy container" {
			var removed domain.Facts
			var new domain.Facts
			if !ev.Success {
				new, removed, err = analyzeDeployPodFailure(ev)
			} else {
				new, removed, err = analyzeDeployPodResult(ev)
			}
			if err != nil {
				slog.Error(fmt.Sprintf("Failed to analyze deploy pod result: %v", err))
			} else {
				factsUpdate.Update(new, removed)
			}
		}
	}

	for _, effect := range ev.TTP.Effects {
		effectUpdate, err := c.ParseEffect(effect, ev.Target, ev.Args, ev.Results...)

		if err != nil {
			if k8sErr, ok := err.(k8s_types.K8sAPIResponseError); ok {
				ev.Success = false
				slog.Error(fmt.Sprintf("K8s API error: '%s': %v", effect, k8sErr))
			} else {
				slog.Error(fmt.Sprintf("Failed to parse effect '%s': %v", effect, err))
			}
			continue
		}

		// 	if strings.HasPrefix(effect, "create") && len(new.Entities) > 0 {
		// 		creatorId := ev.Target.GetId()
		// 		if creator, ok := c.GetEntityById(creatorId); ok {
		// 			for _, entity := range new.Entities {
		// 				new.Relations = append(new.Relations, domain.Created{
		// 					Object:  entity,
		// 					Creator: creator,
		// 				})
		// 			}

		// 		} else {
		// 			slog.Warn(fmt.Sprintf("TTP '%s' created new entity '%s' but creator '%s' is unknown", ttp.ID, new.Entities[0].GetId(), creatorId))
		// 		}
		// 	}
		factsUpdate.Update(effectUpdate.New, effectUpdate.Removed)
	}

	wasValidStep := c.trail.CompleteStep(ev.ID, ev.TTP, ev.Success, results)
	if !wasValidStep && ev.WasCleanup {
		// if cleanup is not associated with a valid step, then it must be a delayed result from a previous campaign
		// => don't influence the current campaign
		return nil, nil
	}
	if !ev.Success {
		new, removed, err := analyzeFailedTTPExecution(ev)

		if err != nil {
			slog.Error(fmt.Sprintf("Failed to analyze failed TTP execution: %v", err))
		}
		factsUpdate.Update(new, removed)
	}
	err = c.bus.Publish(ev)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to publish TTPExecuted event: %v", err))
	}

	// generic analyzer for the successful TTP invocation
	new, removed, err := analyzeToolSuccessfullyUsedInTTP(ev)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to analyze changes after TTP execution: %v", err))
	} else {
		factsUpdate.Update(new, removed)
	}

	newFacts, removedFacts, err := c.AnalyzeChanges(factsUpdate.New, factsUpdate.Removed)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to analyze changes after TTP execution: %v", err))
	}
	return c.UpdateFacts(newFacts, removedFacts)
}

func (c *Campaign) onC2Connected(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.C2Connected)

	// builtin C2 is part of Ran C2
	system := domain.NewC2System(ev.Name, ev.Kind)
	system.IPs = append(system.IPs, ev.IP)

	rels := []domain.Relation{}

	c2s := c.GetC2s()
	ran := c2s[0] // Ran is always the first C2
	if system.GetId() != ran.GetId() {
		operatesRel := domain.Operates{Operator: ran, System: system}
		c.AddRelations(operatesRel)
		rels = append(rels, operatesRel)
	}

	return c.UpdateFacts(domain.Facts{Entities: []domain.Entity{system}, Relations: rels}, domain.Facts{})
}
func (c *Campaign) onC2ConnectFailed(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.C2ConnectFailed)

	return domain.ErrorMsg{
		Level: domain.LevelError,
		Msg:   ev.Name + " " + ev.Reason,
	}, nil
}

func (c *Campaign) onNewSession(ev c2.SessionStarted) (domain.Message, error) {
	c.sessions[ev.Session.Id] = ev.Session
	var sys domain.System

	// see if a pod with that name is already known, if so update it or add a new 'system'
	for _, e := range c.kb.GetEntities() {
		if strings.HasSuffix(e.GetId(), "pod/"+ev.Session.Hostname) {
			var ok bool
			if sys, ok = e.(domain.System); ok {
				if ev.Session.IsRoot {
					sys.SetAccessLevel(domain.RootExec)
				} else {
					sys.SetAccessLevel(domain.UserExec)
				}
			} else {
				slog.Warn("onNewSession: Dont know how to update accesslevel of " + e.GetId())
			}
			break
		}
	}
	err := c.syncCapabilities()
	if err != nil {
		slog.Error(err.Error())
	}

	if sys == nil {
		accessLevel := domain.UserExec
		if ev.Session.IsRoot {
			accessLevel = domain.RootExec
		}

		sys = domain.NewSystem(ev.Session.Hostname, ev.Session.Os, accessLevel)
	}

	// TODO: analyze session:
	// ideas:
	// hostname != pod-name -> maybe hostPID/hostIPC etc. flags are set on pod?
	// or maybe the system is a node

	relations := make([]domain.Relation, 0)

	// convert the communication channel to a relationship
	if ev.Session.IsAlive {
		c2Channel := domain.ImplantC2Channel{
			SessionId: ev.Session.Id,
			SourceId:  fmt.Sprintf("%s/%s", "c2", ev.C2Kind),
			Kind:      ev.C2Kind,
			Target:    ev.Session,
			// Target:    sys,
			// Protocol  string
		}
		hasSession := domain.HasC2Session{
			System:  sys,
			Session: ev.Session,
		}

		relations = append(relations, c2Channel, hasSession)
	}

	newFacts := domain.Facts{
		Entities:  []domain.Entity{sys, ev.Session},
		Relations: relations,
	}
	newFacts, removedFacts, err := c.AnalyzeChanges(newFacts, domain.Facts{})
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to analyze changes after TTP execution: %v", err))
	}

	return c.UpdateFacts(newFacts, removedFacts)
}

func (c *Campaign) onSessionClosed(ev c2.SessionClosed) (domain.Message, error) {
	_, ok := c.sessions[ev.Session.Id]
	if !ok {
		return nil, fmt.Errorf("Unknwon session '%s' could not be closed", ev.Session.Id)
	}
	delete(c.sessions, ev.Session.Id)

	return c.UpdateFacts(domain.Facts{}, domain.Facts{Entities: []domain.Entity{ev.Session}})
}

// func (c *Campaign) onEnvVarsExtracted(ctx context.Context, msg domain.Message) (domain.Message, error) {
// 	newFacts, removedFacts, err := analyzeEnvironmentVariables(msg.(domain.EnvVarsExtracted))
// 	if err != nil {
// 		return nil, err
// 	} else {
// 		return c.UpdateFacts(newFacts, removedFacts)
// 	}
// }

func (c *Campaign) onListenerReady(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(c2.ListenerReady)

	c.trail.CompleteStep(ev.CmdId, domain.TTP{}, true, []string{fmt.Sprintf("Listener on C2 '%s' port %d ready", ev.C2Name, ev.Port)})
	c2, ok := c.GetC2(ev.C2Name)
	if !ok {
		// TODO: need eventual consistency, in case C2 connected event arrives later
		return nil, fmt.Errorf("No C2 '%s' found", ev.C2Name)
	}

	var c2IP net.IP
	if len(c2.IPs) > 0 {
		c2IP = c2.IPs[0]
	}

	// make the ID uninque across all C2s by prefixing it with the C2 name
	listenerID := fmt.Sprintf("%s_%s", ev.C2Name, ev.ID)
	listener := domain.Listener{
		ID:         listenerID,
		Name:       ev.Name,
		IP:         c2IP,
		Port:       ev.Port,
		Protocol:   ev.Protocol,
		Redirector: "",
	}
	// listenerID := ev.Name
	c.listeners[listener.ID] = listener
	c2.Listeners[listener.ID] = listener
	// TODO: complete the TTP step, with the given CmdID

	return c.UpdateFacts(
		domain.Facts{Entities: []domain.Entity{c2}},
		// Relations: []domain.Relation{
		// 	domain.ListenesOn{
		// 		C2ID:       c2.GetId(),
		// 		ListenerID: listenerID,
		// 		Port:       int(ev.Port),
		// 	},
		// }},
		domain.Facts{},
	)
}

func (c *Campaign) onListenerStopped(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(c2.ListenerStopped)
	listenerID := fmt.Sprintf("%s_%s", ev.C2Name, ev.ListenerID)

	listener, ok := c.listeners[listenerID]
	delete(c.listeners, listenerID)
	if !ok {
		return nil, fmt.Errorf("Can't stop unknown listener '%s'", ev.Name)
	}

	var c2 domain.C2System
	for _, c2 = range c.GetC2s() {
		if c2.GetId() == fmt.Sprintf("c2/%s", ev.C2Name) {
			delete(c2.Listeners, listener.ID)
			break
		}
	}

	return c.UpdateFacts(
		domain.Facts{Entities: []domain.Entity{c2}},
		domain.Facts{Entities: []domain.Entity{listener}},
	)
}

// func (c *Campaign) onNewK8sResourceCreated(ctx context.Context, msg domain.Message) (domain.Message, error) {
// 	ev := msg.(domain.NewK8sResourceCreated)
// 	slog.Info(fmt.Sprintf("New K8s resource created: %s", ev.Resource))

// 	entities := []domain.Entity{}
// 	relations := make([]domain.Relation, 0)
// 	if ev.CreatorID != "" {
// 		if creator, ok := c.GetEntityById(ev.CreatorID); ok {
// 			relations = append(relations, domain.Created{
// 				Creator: creator,
// 				Object:  ev.Resource,
// 			})
// 		}
// 	}

// 	// TODO: handle this properly depending on the effects case where a rolebinding is created
// 	if binding, ok := ev.Resource.(domain.RoleBinding); ok {
// 		roleEntity, hasRole := c.GetEntityById(binding.RoleID)
// 		role, isRole := roleEntity.(domain.Role)
// 		if hasRole && isRole {
// 			for _, subjectID := range binding.SubjectIDs {
// 				subject, hasSubject := c.GetEntityById(subjectID)
// 				sa, isSa := subject.(domain.ServiceAccount)

// 				if hasSubject && isSa {
// 					relations = append(relations, domain.BindsRole{
// 						Role:    role,
// 						Subject: sa,
// 					})
// 				}
// 			}
// 		} else {
// 			return nil, fmt.Errorf("RoleBinding '%s' references unknown role '%s'", binding.GetId(), binding.RoleID)
// 		}
// 	} else {
// 		entities = append(entities, ev.Resource)
// 	}

// 	return c.UpdateFacts(
// 		NewFacts{
// 			Entities:  entities,
// 			Relations: relations,
// 		}, RemovedFacts{},
// 	)
// }

func (c *Campaign) onPrintGraph(ctx context.Context, msg domain.Message) (domain.Message, error) {
	if kb, ok := c.kb.(*BuiltInKnowledgeBase); ok {
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
