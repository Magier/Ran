package campaign

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"math/rand"
	"reflect"
	"regexp"
	"strconv"
	"strings"
	"text/template"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/core/bus"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/mitre"
	"github.com/iancoleman/strcase"
)

type Campaign struct {
	kb             KnowledgeBase
	trail          AuditTrail
	activeIdentity string
	armory         *armory.Armory
	listeners      map[string]domain.Listener
	sessions       map[string]domain.Session
	identities     map[string]domain.Identity
	bus            bus.MessageBus
}
type NewFacts struct {
	Entities   []domain.Entity
	Relations  []domain.Relation
	Identities []domain.Identity
	Assets     []domain.Asset
}

func (f *NewFacts) Update(new NewFacts) {
	f.Entities = append(f.Entities, new.Entities...)
	f.Assets = append(f.Assets, new.Assets...)
	f.Relations = append(f.Relations, new.Relations...)
	f.Identities = append(f.Identities, new.Identities...)
}

type RemovedFacts struct {
	Entities   []domain.Entity
	Relations  []domain.Relation
	Identities []domain.Identity
}

func (f *RemovedFacts) Update(new RemovedFacts) {
	f.Entities = append(f.Entities, new.Entities...)
	f.Relations = append(f.Relations, new.Relations...)
	f.Identities = append(f.Identities, new.Identities...)
}

func NewCampaign(armory *armory.Armory) *Campaign {
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

func StartCampaign(mb bus.MessageBus, armory *armory.Armory) *Campaign {
	campaign := NewCampaign(armory)
	campaign.bus = mb
	mb.Subscribe(domain.C2Connected{}, campaign.onC2Connected)
	mb.Subscribe(domain.ExecTTP{}, campaign.onExecuteTTP)
	mb.Subscribe(domain.TTPExecuted{}, campaign.onTTPExecuted)
	mb.Subscribe(domain.ResetCampaign{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		// reset only returns an error value, which will be propagated
		return nil, campaign.Reset()
	})
	mb.Subscribe(c2.ListenerReady{}, campaign.onListenerReady)
	mb.Subscribe(c2.ListenerStopped{}, campaign.onListenerStopped)
	mb.Subscribe(c2.SessionStarted{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return campaign.onNewSession(msg.(c2.SessionStarted))
	})
	mb.Subscribe(c2.SessionClosed{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return campaign.onSessionClosed(msg.(c2.SessionClosed))
	})
	mb.Subscribe(domain.ActionSelected{}, campaign.onActionSelected)
	// mb.Subscribe(domain.TokenPermissionsRetrieved{}, campaign.parseSelfSubjectServiceReview)
	mb.Subscribe(domain.PrintGraph{}, campaign.onPrintGraph)
	mb.Subscribe(domain.SaveAttackFlow{}, campaign.onSaveAttackFlow)
	// mb.Subscribe(domain.EnvVarsExtracted{}, campaign.onEnvVarsExtracted)
	return campaign
}

func (c *Campaign) SetTarget(ns, podName string) (domain.Event, error) {
	if ns == "" {
		ns = "default"
	}
	initialPod := domain.NewPod(podName, ns)

	initialAccessRelation := domain.CanAccess{
		SourceId: "c2/Ran",
		TargetId: initialPod.GetId(),
		// Identity:    identity,
		AccessLevel: domain.UserExec,
		PodsExec:    true,
	}
	initialPod.SetAccessLevel(domain.UserExec)

	initialAccessTTP := domain.TTP{
		ID:         "initial_access",
		Name:       "Initial Access",
		Tactic:     mitre.InitialAccess,
		Techniques: []string{},
	}
	ev := domain.ExecTTP{
		CommandImpl: domain.NewCmd(""),
		TTP:         initialAccessTTP,
		Target:      initialPod,
	}
	err := c.trail.AddNewStep(ev)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to add initial step to audit trail: %s", err.Error()))
	} else {
		c.trail.CompleteStep(ev.GetID(), ev.TTP, true, []string{})
	}

	return c.UpdateFacts(NewFacts{
		Entities:  []domain.Entity{initialPod},
		Relations: []domain.Relation{initialAccessRelation},
	}, RemovedFacts{})
}

func (c *Campaign) UpdateFacts(new NewFacts, removed RemovedFacts) (domain.FactsChanged, error) {
	c.AddEntities(new.Entities...)
	c.AddRelations(new.Relations...)

	c.RemoveEntities(removed.Entities...)

	for _, identity := range new.Identities {
		// if there is no active identity, use the first encountered Id as the active oneo
		if c.activeIdentity == "" {
			c.activeIdentity = identity.GetId()
		}
		c.identities[identity.GetId()] = identity
	}

	err := c.syncCapabilities()
	if err != nil {
		slog.Error(err.Error())
	}
	return domain.FactsChanged{
		NewEntities:   new.Entities,
		NewRelations:  new.Relations,
		NewIdentities: new.Identities,

		RemovedEntities:   removed.Entities,
		RemovedRelations:  removed.Relations,
		RemovedIdentities: removed.Identities,
	}, nil
}

func (c *Campaign) Reset() error {
	err := c.kb.Reset()
	c.trail.Reset()
	c.sessions = make(map[string]domain.Session)
	c.listeners = make(map[string]domain.Listener)
	c.identities = make(map[string]domain.Identity)

	_ = c.kb.AddEntity(domain.C2System{Name: "Ran", Kind: "Ran"})

	return err
}

func (c *Campaign) GetEntities() map[string]domain.Entity {
	return c.kb.GetEntities()
}
func (c *Campaign) GetRelations() map[string]domain.Relation {
	return c.kb.GetRelations()
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
	if id == "" {
		return nil, false
	}
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
		return domain.User{}, false
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
func (c *Campaign) GetAuditTrail() AuditTrail {
	return c.trail
}

func (c *Campaign) AddEntities(entities ...domain.Entity) int {
	numChanges, err := c.kb.AddEntities(entities...)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to insert %d entities: %v", len(entities), err))
	}

	// silly workaraound to ensure that all identities are kept up to date
	for _, entity := range entities {
		if identity, ok := entity.(domain.Identity); ok {
			// add/update all identities, that are part of the entity
			if e, ok := c.GetEntityById(identity.GetId()); ok {
				if id, ok := e.(domain.Identity); ok {
					c.identities[identity.GetId()] = id
				}
			}
		}
	}

	return numChanges
}
func (c *Campaign) RemoveEntities(entities ...domain.Entity) int {
	numChanges, err := c.kb.RemoveEntities(entities...)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to remove %d entities: %v", len(entities), err))
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

func (c *Campaign) RemoveRelations(relations ...domain.Relation) int {
	numChanges, err := c.kb.RemoveRelations(relations...)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to remove %d relations: %v", len(relations), err))
	}
	return numChanges
}

func (c Campaign) selectBestCommandVariant(ttp domain.TTP, procedureID string) (domain.Procedure, error) {
	// TODO: select the variant to execute
	// - keep track of tried variants
	// - favor robust C2 over builtin one

	if len(ttp.Procedures) == 0 {
		return domain.Procedure{}, errors.New("No valid Procedure available for TTP " + ttp.GetID())
	}

	for _, proc := range ttp.Procedures {
		if proc.Key == procedureID {
			return proc, nil
		}
	}

	return ttp.Procedures[0], nil
}

func (c Campaign) GroundAction(ttp domain.TTP, targetId, procedureID string, args map[string]string) (domain.Message, error) {
	if args == nil {
		args = make(map[string]string)
	}
	execCmd := domain.ExecTTP{
		CommandImpl: domain.NewCmd(""),
		TTP:         ttp,
		Args:        args,
	}

	// it's a technique on the C2 side to prepare the infrastructure, not in the target environment
	cmdMsg, err := hydrateCommand(ttp, execCmd.ID, args)
	if err != nil {
		if strings.HasPrefix(err.Error(), "No CommandMsg specified") {
			slog.Debug(err.Error())
		} else {
			slog.Warn(err.Error())
		}
	}
	execCmd.CommandMsg = cmdMsg

	execCmd.Procedure, err = c.selectBestCommandVariant(ttp, procedureID)
	if err != nil && cmdMsg == nil {
		return nil, err
	}

	var target domain.Entity
	target, ok := c.kb.GetEntity(targetId)
	if ok {
		execCmd.Target = target
	}

	var execSystem domain.Entity
	if isActionOnRemoteTarget(execCmd.TTP, execCmd.Procedure) {
		execSystem, err = c.getSystemForExecution(ttp, execCmd.Procedure, target)
		if err != nil {
			slog.Error(fmt.Sprintf("Failed to get system for execution: %s", err.Error()))
		}

		if execSystem != nil {
			c2Channel, err := findC2Channel(c.kb, execSystem)
			if err == nil {
				execCmd.C2Channel = c2Channel
			}
		}
	}

	// use the default value for all parameters, if no extra arg is specified
	for _, param := range execCmd.TTP.Params {
		if _, ok := args[param.Name]; !ok {
			args[param.Name] = param.Default
		}
	}
	args, err = c.groundArgs(args, target, execSystem)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to ground args: %s", err.Error()))
	}
	if execCmd.Procedure.Execute.Code != "" {
		// forward the TTP args to the code snippet
		execCmd.Procedure.Execute.Parameters = args
	}
	execCmd.Procedure.Command = c.groundCmdTemplate(execCmd.Procedure.Command, args)

	// safety: warn about any variable, that was not properly grounded
	re := regexp.MustCompile(`\$\{([^}]+)\}`)
	vars := re.FindAllStringSubmatch(execCmd.Procedure.Command, -1)
	for _, v := range vars {
		slog.Warn(fmt.Sprintf("Ungrounded variable '%s' NOT found in command", v[1]))
	}
	return execCmd, nil
}

func (c Campaign) groundArgs(args map[string]string, target, execSystem domain.Entity) (map[string]string, error) {
	if target == nil {
		target = execSystem
	}

	// build a dependency graph between the args and resolve them in that order
	resolver := NewDependencyResolver(args)
	order, err := resolver.GetEvaluationOrder()
	if err != nil {
		return nil, fmt.Errorf("Failed to get evaluation order to ground args: %s", err.Error())
	}

	// TODO: properly set the default values to the most plausible options
	for _, key := range order {
		arg := args[key]
		if key == "NS" || key == "NAMESPACE" || arg == NS_NAME_VAR {
			if ns, ok := target.(domain.Namespaced); ok {
				arg = ns.GetNamespace()
			} else if ns, ok := target.(domain.Namespace); ok {
				arg = ns.GetName()
			} else if target != nil {
				slog.Warn(fmt.Sprintf("Target '%s' is not namespaced, can't set NS variable", target.GetName()))
			} else {
				slog.Warn("No valid target, can't get its NS variable")
			}
		} else if strings.Contains(arg, POD_NAME_VAR) {
			var podName string
			if target == nil {
				slog.Warn("No valid target or execSystem, use 'ran' as fallback for POD_NAME variable")
				podName = "ran"
			} else {
				podName = target.GetName()
			}
			arg = strings.Replace(arg, POD_NAME_VAR, podName, -1)
		} else if key == "ServiceAccount" {
			if strings.Contains(arg, "ns/") && strings.Contains(arg, "/sa/") {
				arg = strings.SplitN(arg, "/", 4)[3]
			}
		} else if key == "TOKEN" {
			if arg != "" { // resolve the name of the identity to its token
				if entity, ok := c.kb.GetEntity(arg); ok {
					if sa, ok := entity.(domain.ServiceAccount); ok {
						arg = sa.Token.Raw
					}
				} else if nsEntity, ok := target.(domain.Namespaced); ok && nsEntity.IsNamespaced() {
					// create dummy service account to produce valid ID
					tmpSa := domain.NewServiceAccount(arg, nsEntity.GetNamespace())
					saEntity, ok := c.kb.GetEntity(tmpSa.GetId())
					if ok {
						sa, ok := saEntity.(domain.ServiceAccount)
						if ok {
							arg = sa.Token.Raw
						}
					}
				}
			} else { // try to find a sane default
				if sa, ok := target.(domain.ServiceAccount); ok {
					arg = sa.Token.Raw
				} else {
					switch sys := execSystem.(type) {
					case domain.Pod:
						if sys.ServiceAccountName != "" {
							tmpSa := domain.NewServiceAccount(sys.ServiceAccountName, sys.GetNamespace())
							saEntity, ok := c.kb.GetEntity(tmpSa.GetId())
							if ok {
								sa, ok := saEntity.(domain.ServiceAccount)
								if ok {
									arg = sa.Token.Raw
								}
							}
						}
					default:
						slog.Warn("No valid ServiceAccount or Pod found to extract the token from")
					}
				}
			}
		} else if strings.ToUpper(key) == "NODE" || strings.ToUpper(key) == "NODENAME" {
			if arg == "" || arg == NODE_NAME_VAR {
				if pod, ok := target.(domain.Pod); ok {
					arg = pod.NodeName
				} else {
					// variable will be set to empty string, K8s decides where to place the pod
					arg = ""
				}
			} else {
				// ensure the node kind prefis is removed
				arg, _ = strings.CutPrefix(arg, "node/")
			}

		} else if strings.Contains(strings.ToUpper(arg), "${SRC.MOUNT_PATH}") {
			sys, ok := execSystem.(domain.Pod)
			if !ok {
				return nil, fmt.Errorf("Can't ground SRC.MOUNTPATH variable, because execSystem is not a Pod: %s", execSystem.GetName())
			}
			for _, vm := range sys.VolumeMounts {
				if vm.IsHostPath {
					arg = vm.MountPoint
					break
				}
			}
		}

		if strings.Contains(arg, RANDOM_VAR) {
			randomNum := strconv.Itoa(rand.Intn(1e5))
			arg = strings.ReplaceAll(arg, RANDOM_VAR, randomNum)
		}

		// resolve any variables which referencing other args
		// TODO: use the proper resolving order, instead of iterating over all the other variables
		for key, value := range args {
			templateVariable := fmt.Sprintf("${%s}", strings.ToUpper(key))
			arg = strings.ReplaceAll(arg, templateVariable, value)
		}

		args[key] = arg
	}
	return args, nil
}

func hydrateCommand(ttp domain.TTP, execID string, args map[string]string) (domain.Command, error) {
	switch cmd := ttp.CommandMsg.(type) {
	case domain.StartListener:
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

func (c Campaign) groundCmdTemplate(cmdTemplate string, variables map[string]string) string {
	tmpl, err := template.New("cmd").Parse(cmdTemplate)
	if err != nil {
		slog.Error("Ground Template", "", err.Error())
		return cmdTemplate
	}
	var buf strings.Builder

	// Convert string "true"/"false" to bool for template execution
	vars := make(map[string]interface{})
	for k, v := range variables {
		switch v {
		case "true":
			vars[k] = true
		case "false":
			vars[k] = false
		default:
			vars[k] = v
		}
	}
	err = tmpl.Execute(&buf, vars)
	if err != nil {
		slog.Error("Ground Template", "", err.Error())
		return cmdTemplate
	}
	cmdTemplate = buf.String()

	if strings.Contains(cmdTemplate, "${API_SERVER}") {
		apiUrl, err := c.GetApiUrl(true)
		if err != nil {
			slog.Error("Ground Template", "", err.Error())
		} else if apiUrl == "" {
			slog.Info("No API Server URL found when grounding command")
		} else {
			cmdTemplate = strings.Replace(cmdTemplate, "${API_SERVER}", apiUrl, -1)
		}
	}
	if strings.Contains(cmdTemplate, "${LISTENER}") {
		listener, ok := c.GetListener(domain.TCP)
		if ok {
			cmdTemplate = inflateListenerTemplate(listener, cmdTemplate)
		} else {
			slog.Info("No suitable listener found!")
		}
	}
	if strings.Contains(cmdTemplate, "${FILESHARE_PORT}") {
		filesharePort, ok := c.GetFileshare()
		if ok {
			p := fmt.Sprint(filesharePort)
			cmdTemplate = strings.ReplaceAll(cmdTemplate, "${FILESHARE_PORT}", p)
		} else {
			slog.Info("No suitable fileshare found!")
		}
	}

	for key, v := range variables {
		templateVariable := fmt.Sprintf("${%s}", strings.ToUpper(key))
		// TOOD: check if for casese where the variable is not set, and if that's a problem
		cmdTemplate = strings.ReplaceAll(cmdTemplate, templateVariable, v)
	}

	return cmdTemplate
}

func (c Campaign) getServiceAccountOwner(sa domain.ServiceAccount) (domain.Pod, bool) {
	// *vomit*
	if owner, ok := sa.GetOwner(); ok {
		if e, ok := c.GetEntityByName(owner.Name, sa.Namespace); ok {
			if pod, ok := e.(domain.Pod); ok {
				return pod, true
			}
		}
	} else if users, err := c.kb.GetIncomingEntities(sa, domain.Uses{}); err == nil {
		if len(users) > 0 {
			user := users[0]
			if pod, ok := user.(domain.Pod); ok {
				return pod, true
			}
		}
	}
	return domain.Pod{}, false
}

// Determine if the TTP will be executed in the target environment, or the operator infrastructure
func isActionOnRemoteTarget(ttp domain.TTP, cmd domain.Procedure) bool {
	if cmd.IsLocalCommand {
		return false
	}

	// TODO: get rid of this approach
	switch ttp.Tactic {
	case mitre.Reconnaissance, mitre.ResourceDevelopment:
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
	// c2, ok := c.GetC2("Ran")
	// if !ok {
	// 	return errors.New("Couldn't retrieve Ran from KG to sync capabilities")
	// }
	pods := c.GetPods()

	for _, identity := range c.identities {
		if perm, ok := identity.Can("create", "pods/exec"); ok {
			for _, p := range pods {
				// can't access any pods that are no longer running
				ns := p.GetNamespace()

				if !p.IsRunning || !perm.IsInScope(ns) {
					continue
				}
				srcId := identity.GetId()

				accessRelations = append(accessRelations, domain.CanAccess{
					SourceId:    srcId,
					TargetId:    p.GetId(),
					Identity:    identity,
					AccessLevel: domain.UserExec,
					PodsExec:    true,
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

func UnpackResourceID(name string) (string, string, string, error) {
	ns := "default"
	kind := "pod"
	var err error
	if strings.Contains(name, "/") {
		parts := strings.Split(name, "/")
		if len(parts) == 2 {
			ns = parts[1]
			kind = "ns"
			name = ns
		} else if parts[0] == "ns" && len(parts) == 4 {
			// it's the ID format `ns/<ns>/<kind>/<podname>`
			ns = parts[1]
			kind = parts[2]
			name = parts[3]
		} else {
			err = fmt.Errorf("invalid target format")
		}
	}

	kind = domain.GetKindFromResourceShortName(kind)
	return ns, kind, name, err
}
