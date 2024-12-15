package campaign

import (
	"bytes"
	"context"
	"fmt"
	"log/slog"
	"strings"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/domain"
	"github.com/dominikbraun/graph/draw"
	"github.com/goccy/go-graphviz"
)

func (c *Campaign) onC2Connected(ctx context.Context, msg domain.Message) (domain.Message, error) {
	ev := msg.(domain.C2Connected)
	system := domain.C2System{
		Kind: ev.Kind,
		Name: ev.Name,
		IP:   ev.IP,
	}

	return domain.NewFacts{
		Entities: []domain.Entity{system},
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

	// convert the communication channel to a relationship
	c2Channel := domain.ImplantC2Channel{
		SessionId: ev.Session.Id,
		Source:    fmt.Sprintf("%s/%s", "c2", ev.C2Kind),
		Kind:      ev.C2Kind,
		Target: domain.Target{
			Id:     system.GetId(),
			Name:   system.GetName(),
			Entity: system,
		},
		// 	Target    Target
		// Protocol  string
	}

	msg := domain.NewFacts{
		Entities:  []domain.Entity{system},
		Relations: []domain.Relation{c2Channel},
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

func (c *Campaign) onNewFacts(ctx context.Context, msg domain.Message) (domain.Message, error) {
	// TODO: properly track how many changes the update contained
	numChanges := 0
	ev := msg.(domain.NewFacts)
	numChanges += c.AddEntities(ev.Entities...)
	numChanges += c.AddRelations(ev.Relations...)

	for _, identity := range ev.Identities {
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
		if err := g.RenderFilename(ctx, graph, graphviz.PNG, "topo.png"); err != nil {
			return nil, err
		}
	}
	return nil, nil
}
