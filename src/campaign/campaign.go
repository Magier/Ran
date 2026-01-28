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
	lastExecSystem domain.System
}

type factsUpdate struct {
	New     domain.Facts
	Removed domain.Facts
}

func (f *factsUpdate) Update(new domain.Facts, removed domain.Facts) {
	f.New.Update(new)
	f.Removed.Update(removed)
}

func NewCampaign(armory *armory.Armory) *Campaign {
	kg := InitGraph()

	_ = kg.AddEntity(domain.NewC2System("Ran", "Ran"))
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
	mb.Subscribe(domain.C2ConnectFailed{}, campaign.onC2ConnectFailed)
	mb.Subscribe(domain.ExecTTP{}, campaign.onExecuteTTP)
	mb.Subscribe(domain.ResetCampaign{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return nil, campaign.Reset()
	})
	mb.Subscribe(c2.TTPExecuted{}, campaign.onC2TTPExecuted)
	mb.Subscribe(c2.ListenerReady{}, campaign.onListenerReady)
	mb.Subscribe(c2.ListenerStopped{}, campaign.onListenerStopped)
	mb.Subscribe(c2.SessionStarted{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return campaign.onNewSession(msg.(c2.SessionStarted))
	})
	mb.Subscribe(c2.SessionClosed{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		return campaign.onSessionClosed(msg.(c2.SessionClosed))
	})
	// mb.Subscribe(domain.ActionSelected{}, campaign.onActionSelected)
	// mb.Subscribe(domain.TokenPermissionsRetrieved{}, campaign.parseSelfSubjectServiceReview)
	mb.Subscribe(domain.PrintGraph{}, campaign.onPrintGraph)
	mb.Subscribe(domain.SaveAttackFlow{}, campaign.onSaveAttackFlow)
	// mb.Subscribe(domain.EnvVarsExtracted{}, campaign.onEnvVarsExtracted)
	return campaign
}

func (c *Campaign) SetTarget(ns, podName string) (domain.ExecTTP, error) {
	if ns == "" {
		ns = "default"
	}
	initialPod := domain.NewPod(podName, ns)
	initialPod.SetAccessLevel(domain.UserExec)

	args := map[string]string{
		"Name":      podName,
		"Namespace": ns,
	}

	if ttp, ok := c.armory.GetTTP("initial-access-pod-exec"); ok {
		ev := domain.ExecTTP{
			CommandImpl: domain.NewCmd(""),
			TTP:         ttp,
			Target:      initialPod,
			Args:        args,
		}
		return ev, nil
	}
	return domain.ExecTTP{}, fmt.Errorf("No initial access TTP found in armory")
}

func (c *Campaign) ExecuteAction(ctx context.Context, ev domain.ActionSelected) error {
	ttp, ok := c.armory.GetTTP(ev.ActionID)
	if !ok {
		slog.Error(fmt.Sprintf("No TTP with ID '%s' found!", ev.ActionID))
		return NewTTPNotFoundError(ev.ActionID)
	}

	var execCmd domain.ExecTTP
	var err error

	if ttp.Tactic == mitre.InitialAccess {
		ns := ev.Args["Namespace"]
		name := ev.Args["TargetName"]

		// fallback to targetID if no explicit targetname is provided
		if name == "" && ev.TargetID != "cluster" {
			name = ev.TargetID
		}

		if strings.HasPrefix(name, "ns/") {
			var err error
			ns, _, name, err = UnpackResourceID(name)
			if err != nil {
				slog.Error(fmt.Sprintf("Failed to unpack resource ID '%s': %v", name, err))
				return err
			}
		}

		execCmd, err = c.SetTarget(ns, name)
		if err == nil {
			execCmd.ID = ev.CmdId
		}
	} else {
		execCmd, err = c.GroundAction(ttp, ev.TargetID, ev.ProcedureID, ev.Args)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not ground action: %v\n", err))
			return err
		} else {
			// remember the system we are executing from, to continue the attack from there
			c.lastExecSystem, ok = execCmd.Target.(domain.System)
			execCmd.ID = ev.CmdId
			if !ok {
				slog.Warn("Executed TTP target is not a system, can't continue the attack from there")
				c.lastExecSystem = nil
			}
		}
	}

	if err = c.bus.Publish(execCmd); err != nil {
		slog.Error(fmt.Sprintf("Failed to publish command: %v", err))
		return err
	}
	return err
}

func (c *Campaign) UpdateFacts(new domain.Facts, removed domain.Facts) (domain.FactsChanged, error) {
	c.AddEntities(new.Entities...)
	c.AddRelations(new.Relations...)

	c.RemoveEntities(removed.Entities...)
	c.RemoveRelations(removed.Relations...)

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
		New:     new,
		Removed: removed,
	}, nil
}

func (c *Campaign) Reset() error {
	c.cleanupSteps()

	err := c.kb.Reset()
	if err != nil {
		return err
	}

	c.trail.Reset()
	c.sessions = make(map[string]domain.Session)
	c.listeners = make(map[string]domain.Listener)
	c.identities = make(map[string]domain.Identity)

	err = c.kb.AddEntity(domain.NewC2System("Ran", "Ran"))
	if err != nil {
		return err
	}
	return c.bus.Publish(domain.CampaignReset{})
}

func (c *Campaign) cleanupSteps() {
	executedSteps := c.trail.GetSteps()
	// cleanup after all the steps in the reverse order
	for i := len(executedSteps) - 1; i >= 0; i-- {
		step := executedSteps[i]
		// maybe a bit of a workaround to turn a cleanup procedure into a dedicated TTP
		// but it allows easy re-use of the same execution flows, to avoid dedicated handling
		if cleanup := step.TTP.Cleanup; cleanup.Command != "" {
			cleanupTTP := domain.TTP{
				ID:         fmt.Sprintf("%s_cleanup", step.TTP.ID),
				Name:       fmt.Sprintf("%s Cleanup", step.TTP.Name),
				Tactic:     step.TTP.Tactic,
				Techniques: step.TTP.Techniques,
				Params:     step.TTP.Params,
				Procedures: []domain.Procedure{step.TTP.Cleanup},
			}

			execCmd, err := c.GroundAction(cleanupTTP, step.Target.GetId(), cleanup.Key, step.Args)
			execCmd.IsCleanup = true
			if err != nil {
				slog.Error(fmt.Sprintf("Failed to ground cleanup action for step '%s': %v", step.ID, err))
			}

			if err = c.bus.Publish(execCmd); err != nil {
				slog.Error(fmt.Sprintf("Failed to publish cleanup action for step '%s': %v", step.ID, err))
			}
		}
	}
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
		slog.Info("No clear API IP known, using service DNS instead")
		return "https://kubernetes.default.svc.cluster.local", nil
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

func (c Campaign) selectBestProcedure(ttp domain.TTP, procedureID string) (domain.Procedure, error) {
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

func (c Campaign) GroundAction(ttp domain.TTP, targetId, procedureID string, args map[string]string) (domain.ExecTTP, error) {
	if args == nil {
		args = make(map[string]string)
	}

	// sanity check: ensure all params are known
	for argName := range args {
		found := false
		for _, param := range ttp.Params {
			if param.Name == argName {
				found = true
				break
			}
		}
		if !found {
			slog.Warn(fmt.Sprintf("TTP '%s' has no parameter named '%s'", ttp.Name, argName))
		}
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

	execCmd.Procedure, err = c.selectBestProcedure(ttp, procedureID)
	if err != nil && cmdMsg == nil {
		return execCmd, err
	}

	var target domain.Entity
	if targetId != "" && targetId != "cluster" {
		var ok bool
		target, ok = c.kb.GetEntity(targetId)
		if !ok {
			return execCmd, NewTargetNotFoundError(targetId)
		}
		execCmd.Target = target
	}

	var execSystem domain.Entity
	if isActionOnRemoteTarget(execCmd.TTP, execCmd.Procedure) {
		execSystem, err = c.getSystemForExecution(execCmd.Procedure, target)
		if err != nil {
			slog.Error(fmt.Sprintf("Failed to get system for execution: %s", err.Error()))
		}

		if execSystem != nil {
			c2Channel, err := findC2Channel(c.kb, execSystem)
			if err == nil {
				execCmd.C2Channel = c2Channel
			} else {
				slog.Error(err.Error())
			}
		}
	} else {
		slog.Warn("action is not on remote target!")
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

	// build the final command
	if finalCmd, err := c.buildFinalCommand(execCmd.C2Channel, execCmd.Procedure); err == nil {
		execCmd.Procedure.Command = finalCmd
	} else {
		slog.Error(fmt.Sprintf("Failed to build final command: %s", err.Error()))
	}

	// safety: warn about any variable, that was not properly grounded
	re := regexp.MustCompile(`\$\{([^}]+)\}`)
	vars := re.FindAllStringSubmatch(execCmd.Procedure.Command, -1)
	for _, v := range vars {
		slog.Warn(fmt.Sprintf("Ungrounded variable '%s' NOT found in command", v[1]))
	}

	return execCmd, nil
}

func groundUsedTool(proc domain.Procedure, sys domain.System) (domain.Procedure, error) {
	toolName := proc.GetTool()
	if binPath := sys.GetBinary(toolName); binPath != "" && binPath != toolName {
		// in the command a must be is a stand-alone string, so add spaces around it to avoid partial replacements
		re := regexp.MustCompile(fmt.Sprintf(`\b%s\b`, regexp.QuoteMeta(toolName)))
		proc.Command = re.ReplaceAllString(proc.Command, binPath)
	}
	return proc, nil
}

func (c Campaign) buildFinalCommand(ch domain.C2Channel, proc domain.Procedure) (string, error) {
	// if sys, ok := execSystem.(domain.System); ok {
	// 	if execCmd.Procedure, err = groundUsedTool(execCmd.Procedure, sys); err != nil {
	// 		slog.Error(fmt.Sprintf("Failed to ground used tool in command: %s", err.Error()))
	// 	}
	// }
	if ch == nil {
		return proc.Command, nil
	}
	// recursive call to get to the final channel and build the wrapping commands backwards
	var err error
	if nextChannel := ch.GetNextChannel(); nextChannel != nil {
		proc.Command, err = c.buildFinalCommand(ch.GetNextChannel(), proc)
		if err != nil {
			return "", err
		}
		// TODO: fix temporary workaround and generalize to TTPs using the right one
		proc.Command = nextChannel.GetCommandEnvelope(proc.Command)
		if strings.HasPrefix(proc.Command, "kubectl") {
			// TODO: fix temporary workaround: set tool so it's properly grounded below
			proc.Tool = "kubectl"
		}
	}

	target := ch.GetTarget()
	if sys, ok := target.(domain.System); ok {
		proc, err = groundUsedTool(proc, sys)
		if err != nil {
			return "", err
		}
	}

	return proc.Command, nil
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
				slog.Warn("No valid target -> using `default` namespace")
				arg = "default"
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
				} else if gcpSa, ok := target.(domain.GCPServiceAccount); ok {
					arg = gcpSa.Token.Token
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
		} else if strings.Contains(arg, "${LISTENER}") {
			listener, ok := c.GetListener(domain.TCP)
			if ok {
				arg = strings.ReplaceAll(arg, "${LISTENER}", listener.IP.String())
			} else {
				slog.Warn("No suitable listener found!")
			}
		} else if strings.Contains(arg, "${LISTENER_PORT}") {
			listener, ok := c.GetListener(domain.TCP)
			if ok {
				p := fmt.Sprint(listener.Port)
				arg = strings.ReplaceAll(arg, "${LISTENER_PORT}", p)
			} else {
				slog.Warn("No suitable listener found!")
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
	case domain.StopListener:
		if listenerID, ok := args["ListenerID"]; ok {
			// the ID had the C2 name prefixed to make it unique across C2s
			parts := strings.SplitN(listenerID, "_", 2)
			cmd.ID = parts[1]
		} else {
			return nil, errors.New("No listenerID specified to stop the listener")
		}
		return cmd, nil
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

func (c Campaign) GetK8sCluster() (domain.Cluster, bool) {
	for _, e := range c.kb.GetEntities() {
		if cluster, ok := e.(domain.Cluster); ok {
			return cluster, true
		}
	}
	return domain.Cluster{}, false
}

func (c Campaign) GetCloudServiceProvider() (domain.CloudEnvironment, bool) {
	for _, e := range c.kb.GetEntities() {
		if csp, ok := e.(domain.CloudEnvironment); ok {
			return csp, true
		}
	}
	return domain.CloudEnvironment{}, false
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

	template = strings.ReplaceAll(template, "${LISTENER}", dst)
	return template
}

func UnpackResourceID(name string) (string, string, string, error) {
	if name == "" {
		return "", "", "", fmt.Errorf("resource ID is empty")
	}

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

func (c *Campaign) getSystems(includeKnown, includeUnknown bool) []domain.System {
	systems := make([]domain.System, 0)
	for _, e := range c.kb.GetEntities() {
		if sys, ok := e.(domain.System); ok {
			switch sys.(type) {
			case domain.UnknownSystem:
				if includeUnknown {
					systems = append(systems, sys)
				}
			default:
				if includeKnown {
					systems = append(systems, sys)
				}
			}
		}
	}
	return systems
}
