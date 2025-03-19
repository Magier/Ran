package campaign

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"os"
	"reflect"
	"strconv"
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
	"github.com/Magier/Ran/parsers"
	"github.com/dominikbraun/graph/draw"
	"github.com/goccy/go-graphviz"
	"github.com/iancoleman/strcase"
)

func (c *Campaign) onActionSelected(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.ActionSelected)

	ttp, ok := c.armory.GetTTP(ev.ActionID)
	if !ok {
		msg := fmt.Sprintf("No TTP with ID '%s' found!", ev.ActionID)
		slog.Error(msg)
		return nil, fmt.Errorf(msg)
	}

	msg, err := c.GroundAction(ttp, ev.TargetID, ev.Args)
	if err != nil {
		slog.Error(fmt.Sprintf("Could not ground action: %v\n", err))
	}
	return msg, err
}

func hydrateCommand(ttp domain.TTP, execID string, args map[string]string) (domain.Command, error) {
	switch cmd := ttp.CommandMsg.(type) {
	case domain.StartListener:
		// t := reflect.TypeOf(cmd)
		// ptr := reflect.New(t)
		// val := ptr.Elem()
		// inf := val.Interface()
		// var _ = inf

		c := reflect.ValueOf(&cmd).Elem()
		if c.Kind() != reflect.Struct {
			return nil, errors.New("Can't ground PreAction, because cmd is not a struct!")
		}

		for name, v := range args {
			name = strcase.ToCamel(name)
			f := reflect.ValueOf(v)
			field := c.FieldByName(name)

			if !field.CanSet() {
				continue
			}

			switch field.Kind() {
			case reflect.String:
				field.SetString(f.String())
			case reflect.Uint:
				val, err := strconv.ParseUint(v, 10, 64)
				if err != nil {
					return nil, fmt.Errorf("failed to convert string to uint: %v", err)
				}
				field.SetUint(val)
			case reflect.Float64:
				field.SetFloat(f.Float())
			case reflect.Slice:
				field.Set(f.Slice(0, f.Len()))
			}
		}
		cmd.SetID(execID)
		// TODO populate the arguments
		return cmd, nil
		// default:
	case nil:
		return nil, errors.New("No CommandMsg specified for TTP: " + ttp.GetTitle())

	}

	return nil, nil
}
func (c *Campaign) onExecuteTTP(ctx context.Context, msg domain.Message) (domain.Message, error) {
	cmd := msg.(domain.ExecTTP)
	err := c.trail.AddNewStep(cmd)
	return nil, err
}

func (c *Campaign) onTTPExecuted(ctx context.Context, msg domain.Message) (domain.Message, error) {
	cmd := msg.(domain.TTPExecuted)
	ttp := cmd.TTP

	c.trail.CompleteStep(cmd.ID, cmd.TTP, true, cmd.TTP.Description)

	// post processing will yield the final message
	if fn := parsers.GetParser(ttp.Parser); fn != nil {
		event, err := fn(cmd.Target, cmd.Results...)
		return event, err
	}

	if len(ttp.Effects) > 1 {
		slog.Info(fmt.Sprintf("TTP has %d effects; using only first one", len(ttp.Effects)))
	}
	for _, effect := range ttp.Effects {
		return parseEffect(effect, cmd.Target, cmd.Results...), nil
	}
	return nil, nil
}

func (c *Campaign) onTTPFailed(ctx context.Context, msg domain.Message) (domain.Message, error) {
	cmd := msg.(domain.TTPFailed)

	if strings.Contains(cmd.Reason, ": not found") {
		// TODO: parse the binary name and add it as information, that the targeted system has no binary
	}

	c.trail.CompleteStep(cmd.ID, cmd.TTP, false, cmd.Reason)
	slog.Error(cmd.Reason)
	return nil, nil
}

func (c *Campaign) onC2Connected(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.C2Connected)
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

	return domain.FactsChanged{
		NewEntities:  []domain.Entity{system},
		NewRelations: rels,
	}, nil
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

	msg := domain.FactsChanged{
		NewEntities:  []domain.Entity{system, ev.Session},
		NewRelations: []domain.Relation{c2Channel, hasSession},
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

func (c *Campaign) onEnvVarsExtracted(ctx context.Context, msg domain.Message) (domain.Message, error) {
	return analyzeEnvironmentVariables(msg.(domain.EnvVarsExtracted))
}
func (c *Campaign) onServiceAccountTokenExtracted(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.ServiceAccountTokenExtracted)
	msg, err := analyzeServiceAccountToken(ev.Token)
	return msg, err
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

	return domain.FactsChanged{
		NewEntities: []domain.Entity{sa},
	}, nil
}

func (c *Campaign) onFactsChanged(ctx context.Context, msg domain.Message) (domain.Message, error) {
	// TODO: properly track how many changes the update contained
	numChanges := 0
	ev := msg.(domain.FactsChanged)
	numChanges += c.AddEntities(ev.NewEntities...)
	numChanges += c.AddRelations(ev.NewRelations...)

	for _, identity := range ev.NewIdentities {
		// if there is no active identity, use the first encountered Id as the active oneo
		if c.activeIdentity == "" {
			c.activeIdentity = identity.Name
		}
		c.identities[identity.Name] = identity
	}

	err := c.syncCapabilities()
	if err != nil {
		slog.Error(err.Error())
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
	id := ev.Name

	c.trail.CompleteStep(ev.CmdId, domain.TTP{}, true, fmt.Sprintf("Listener on C2 '%s' port %d ready", ev.C2Server, ev.Port))
	c2, ok := c.GetC2(ev.C2Server)
	if !ok {
		return nil, fmt.Errorf("No C2 '%s' found", ev.C2Server)
	}

	var c2IP net.IP
	if len(c2.IPs) > 0 {
		c2IP = c2.IPs[0]
	}

	c.listeners[id] = domain.Listener{
		ID:         id,
		IP:         c2IP,
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

func parseEffect(effect string, source domain.Entity, args ...any) domain.Message {
	if len(args) == 0 {
		slog.Warn("Can't parse effect %s because there are no arguments")
		return nil
	}

	entities := []domain.Entity{}
	switch strings.ToLower(effect) {
	// TODO: set these 'attribute' effects via reflection
	case "target.ip":
		if pod, ok := source.(domain.Pod); ok {
			ips := []net.IPAddr{}
			if res, ok := args[0].(string); ok {
				for _, ip := range strings.Split(res, " ") {
					parsedIP := net.ParseIP(ip)
					if parsedIP == nil {
						slog.Error("Failed to parse IP")
						break
					}
					ips = append(ips, net.IPAddr{IP: parsedIP})
				}
			}
			pod.IPs = ips
			entities = append(entities, pod)
		}
	case "k8s.podlist":
		if res, ok := args[0].(string); ok {
			podList, err := k8s.ParsePodList(res)
			if err != nil {
				slog.Error(fmt.Sprintf("Could not parse PodList: %v", err))
			}

			for _, pod := range podList.Items {
				entities = append(entities, domain.NewPodFromK8sSpec(pod))
			}
		}
	}
	return domain.FactsChanged{NewEntities: entities}
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
