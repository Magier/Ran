package campaign

import (
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"strings"

	"github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
	k8s_types "github.com/Magier/Ran/k8sclient/types"
)

func GetParser(parserName string) domain.ParserFn {
	switch parserName {
	case "rawServiceaccountToken":
		return HandleSaTokenRead
	case "environmentVariables":
		return HandleEnvVarResult
	case "selfSubjectReview", "authCanI":
		return HandleSelfSubjectReviewResult
	case "newContainer":
		return HandleNewContainer
	case "newRole":
		return HandleNewRole
	case "newRoleBinding":
		return HandleNewRoleBinding
	case "newCronJob":
		return HandleNewCronJob
	default:
		slog.Warn(fmt.Sprintf("Parser '%s' not implemented!", parserName))
	}
	return nil
}

func HandleEnvVarResult(ev domain.TTPExecuted, source domain.Entity, args ...string) (domain.Event, error) {
	if len(args) == 0 {
		return nil, errors.New("No environment variables received!")
	}
	stderr := args[1]
	if stderr != "" {
		return nil, errors.New(stderr)
	}

	stdout := args[0]
	vars := make(map[string]string)

	var sep = "\n"
	if strings.Contains(stdout, "\x00") {
		sep = "\x00"
	}

	for _, l := range strings.Split(stdout, sep) {
		k, v, ok := strings.Cut(l, "=")
		if ok {
			vars[k] = v
		}
	}

	return domain.EnvVarsExtracted{
		Source: source,
		Vars:   vars,
	}, nil
}

func HandleSaTokenRead(ev domain.TTPExecuted, source domain.Entity, args ...string) (domain.Event, error) {
	if len(args) == 0 {
		return nil, fmt.Errorf("No SA token provided as argument")
	}

	var token string = args[0]
	if len(token) == 0 {
		return nil, fmt.Errorf("Empty SA token can't be decoded")
	}
	if len(args) > 1 && args[1] != "" {
		slog.Warn(fmt.Sprintf("Sa Token Read expects exactly 1 argument - received %d", len(args)))
	}

	// clean it up if necessary
	if strings.Contains(args[0], "\n") {
		for _, part := range strings.Split(args[0], "\n") {
			// naive heuristic to find the token: they are Base64 encoded which starts with "{"
			// and has 2 '.' to separate the parts of the JWT token
			if strings.Contains(part, "ey") && strings.Contains(part, ".") {
				token = part
				break
			}
		}
	}

	// TODO: the sourceID does not have to be the pod that actually mounts the SA Token
	return domain.ServiceAccountTokenExtracted{
		SourceSystemId: source.GetId(),
		Token:          token,
	}, nil
}

func HandleSelfSubjectReviewResult(ev domain.TTPExecuted, source domain.Entity, args ...string) (domain.Event, error) {
	// try parse JSON
	if len(args) == 0 {
		return nil, fmt.Errorf("No data")
	}
	var result k8s_types.SelfSubjectRulesReview
	var err error

	data := args[0]

	// Check if jsonData is valid JSON, otherwise try to parse as pretty-printed table
	if json.Valid([]byte(data)) {
		err = json.Unmarshal([]byte(data), &result)
		if err != nil {
			return nil, fmt.Errorf("Failed to unmarshal JSON: %w", err)
		}
	} else {
		slog.Warn("Input is not valid JSON, attempting to parse as pretty-printed SelfSubjectRulesReview")
		result, err = parsePrettySelfSubjectRulesReview(data)
		if err != nil {
			return nil, fmt.Errorf("Failed to parse pretty-printed SelfSubjectRulesReview: %w", err)
		}
	}

	if result.Code >= 400 {
		return domain.TTPExecuted{
			Success: false,
			Results: []string{result.Message},
		}, nil
	}

	if result.Status.Incomplete {
		slog.Warn("Results from SelfSubjectRulesReview are incomplete!")
	}

	sa, ok := source.(domain.ServiceAccount)
	if !ok {
		slog.Warn("the source of the SubjectReviewResult is not a valid ServiceAccount!")
	}
	return domain.TokenPermissionsRetrieved{
		TokenName:        source.GetName(),
		ServiceAccount:   sa,
		ResourceRules:    result.Status.ResourceRules,
		NonResourceRules: result.Status.NonResourceRules,
	}, nil
}

func parsePrettySelfSubjectRulesReview(data string) (k8s_types.SelfSubjectRulesReview, error) {
	resRules := []k8s_types.ResourceRule{}
	nonResRules := []k8s_types.NonResourceRule{}
	lines := strings.Split(data, "\n")
	var row [][]string

	// Skip the header and parse remaining lines
	for _, line := range lines[1:] {
		fields := strings.SplitN(line, "[", 4)
		// clean every cell by dropping the closing ']' and trimming whitespace
		row = make([][]string, 4)
		for i := range fields {
			f := strings.TrimSuffix(strings.TrimSpace(fields[i]), "]")
			if f == "" {
				row[i] = []string{}
			} else {
				row[i] = strings.Split(f, " ")
			}
		}

		// empty "Resources" column means it's a NonResourceRule
		if len(row[0]) == 0 {
			nonResRules = append(nonResRules, k8s_types.NonResourceRule{
				NonResourceURLs: row[0],
				Verbs:           row[3],
			})
		} else {
			resRules = append(resRules, k8s_types.ResourceRule{
				APIGroups:     row[0], // assuming default API group
				Resources:     row[0],
				ResourceNames: row[2],
				Verbs:         row[3],
			})
		}
	}
	// Note: full JSON resopnse is impossible to reproduce, as the grouping is not based on the data itself
	return k8s_types.SelfSubjectRulesReview{
		Status: k8s_types.SubjectRulesReviewStatus{
			ResourceRules:    resRules,
			NonResourceRules: nonResRules,
		},
	}, nil
}

func HandleNewContainer(ev domain.TTPExecuted, source domain.Entity, args ...string) (domain.Event, error) {
	numArgs := len(args)
	if numArgs == 0 {
		return nil, fmt.Errorf("No data")
	}
	if numArgs != 3 {
		return nil, fmt.Errorf("Expected podName, namespaceName and podConfig; got %d args instead", numArgs)
	}

	podName := args[0]
	nsName := args[1]
	ns := domain.Namespace{Name: nsName}
	p := domain.NewPod(podName, nsName)
	// TODO: marshal the podConfig
	var cfg domain.PodConfig
	err := json.Unmarshal([]byte(args[2]), &cfg)
	if err != nil {
		return nil, fmt.Errorf("Failed to unmarshal PodConfig JSON: %w", err)
	}
	// cfgJson := args[2].(domain.PodConfig)

	p.HostIPC = domain.NewProbBool(cfg.HostIPC)
	p.HostPID = domain.NewProbBool(cfg.HostPID)
	p.HostNetwork = domain.NewProbBool(cfg.HostNetwork)
	p.Privileged = domain.NewProbBool(cfg.Privileged)

	slog.Error(fmt.Sprintf("Creating new pod %s in namespace %s is not yet properly implemented! FIX NEEDED!", p.Name, ns.Name))
	return domain.NewPodDeployed{
		Pod:       p,
		Namespace: ns,
	}, nil
}

func HandleNewCronJob(ev domain.TTPExecuted, source domain.Entity, args ...string) (domain.Event, error) {
	numArgs := len(args)
	if numArgs == 0 {
		return nil, fmt.Errorf("No data")
	}

	podName := args[0]
	nsName := args[1]
	if nsName == "" {
		if src, ok := source.(domain.K8sEntity); ok {
			nsName = src.GetNamespace()
		} else {
			return nil, fmt.Errorf("source does not have a namespace")
		}
	}
	ns := domain.Namespace{Name: nsName}
	p := domain.NewPod(podName, nsName)

	if len(args) >= 3 {
		// TODO: marshal the podConfig
		var cfg domain.PodConfig
		err := json.Unmarshal([]byte(args[2]), &cfg)
		if err != nil {
			return nil, fmt.Errorf("Failed to unmarshal PodConfig JSON: %w", err)
		}
		// cfgJson := args[2].(domain.PodConfig)

		p.HostIPC = domain.NewProbBool(cfg.HostIPC)
		p.HostPID = domain.NewProbBool(cfg.HostPID)
		p.HostNetwork = domain.NewProbBool(cfg.HostNetwork)
		p.Privileged = domain.NewProbBool(cfg.Privileged)
	}

	// TODO: this should also add the new CronJob to the knowledge base, which owns this pod

	slog.Error(fmt.Sprintf("Creating new pod %s in namespace %s is not yet properly implemented! FIX NEEDED!", p.Name, ns.Name))
	return domain.NewPodDeployed{
		Pod:       p,
		Namespace: ns,
	}, nil
}

func HandleNewRole(ev domain.TTPExecuted, source domain.Entity, args ...string) (domain.Event, error) {
	// TODO: check if the actual TTP execution failed, because the role already exists
	// -> overall, the intended effects are met, but it may be a confiict (e.g. name collision), for downstream TTPs
	if strings.Contains(args[0], "Error from server (Forbidden)") {
		// "command terminated with exit code 1: 'Error from server (Forbidden): roles.rbac.authorization.k8s.io \"nsadmin\" is forbidden: user \"system:serviceaccount:dev:developer\" (groups=[\"system:serviceaccounts\" \"system:serviceaccounts:dev\" \"system:authenticated\"]) is attempting to grant RBAC permissions not currently held:\n{APIGroups:[\"\"], Resources:[\"*\"], Verbs:[\"*\"]}\n'"
		if strings.Contains(args[0], "attempting to grant RBAC permissions not currently held") {
			return nil, errors.New(args[0])
		}
	}

	name := ev.Args["ROLE_NAME"]
	if strings.Contains(args[0], "already exists") {
		slog.Info(fmt.Sprintf("Role '%s' already exists: %s", name, args[0]))
	}

	var ns string
	var creator domain.ServiceAccount

	if sa, ok := ev.Target.(domain.ServiceAccount); ok {
		ns = sa.GetNamespace()
		creator = sa
	}

	role := domain.Role{
		K8sEntity: domain.K8sEntity{
			Name:      name,
			Namespace: ns,
			Kind:      "Role",
		},
	}

	myId := role.GetId()
	var _ = myId

	return domain.NewK8sResourceCreated{
		Resource:  role,
		CreatorID: creator.GetId(),
	}, nil
}

func HandleNewRoleBinding(ev domain.TTPExecuted, source domain.Entity, args ...string) (domain.Event, error) {
	if strings.Contains(args[0], "Error from server (Forbidden)") {
		// "command terminated with exit code 1: 'Error from server (Forbidden): roles.rbac.authorization.k8s.io \"nsadmin\" is forbidden: user \"system:serviceaccount:dev:developer\" (groups=[\"system:serviceaccounts\" \"system:serviceaccounts:dev\" \"system:authenticated\"]) is attempting to grant RBAC permissions not currently held:\n{APIGroups:[\"\"], Resources:[\"*\"], Verbs:[\"*\"]}\n'"
		if strings.Contains(args[0], "attempting to grant RBAC permissions not currently held") {
			return nil, errors.New(args[0])
		}
	}

	name := ev.Args["BINDING_NAME"]
	if strings.Contains(args[0], "already exists") {
		slog.Info(fmt.Sprintf("RoleBinding '%s' already exists: %s", name, args[0]))
	}

	ns := ev.Args["NAMESPACE"]
	roleID := fmt.Sprintf("ns/%s/role/%s", ns, ev.Args["ROLE_NAME"])
	subjectID := fmt.Sprintf("ns/%s/sa/%s", ns, ev.Args["SUBJECT"])

	binding := domain.RoleBinding{
		K8sEntity: domain.K8sEntity{
			Name:      name,
			Namespace: ns,
			Kind:      "RoleBinding",
		},
		RoleID:     roleID,
		SubjectIDs: []string{subjectID},
	}

	// TODO infer the proper creator
	// creatorName := fmt.Sprintf("ns/%s/sa/%s", ns, ev.Args["TOKEN"])
	return domain.NewK8sResourceCreated{
		Resource: binding,
		// CreatorID: creatorName,
	}, nil
}

func ParseSecretList(jsonStr string) ([]domain.K8sSecret, error) {
	secretList, err := k8s.ParseSecretList(jsonStr)
	if err != nil {
		return nil, fmt.Errorf("Failed to parse SecretList: %w", err)
	}

	secrets := make([]domain.K8sSecret, 0, len(secretList.Items))
	for _, item := range secretList.Items {
		secrets = append(secrets, domain.NewSecretFromK8sSpec(item))
	}

	return secrets, nil
}

func ParseEffect(effect string, source domain.Entity, args map[string]string, results ...string) (NewFacts, RemovedFacts) {
	if len(results) == 0 {
		slog.Warn("Can't parse effect %s because there are no arguments")
		return NewFacts{}, RemovedFacts{}
	}

	// alreadyExists := false
	if strings.Contains(results[0], "already exists") {
		// alreadyExists = true
		slog.Info(fmt.Sprintf("Parsing Effect: entity '%s' already exists", effect))
	}

	isRemoveEffect := strings.HasPrefix(effect, "delete")
	effect = strings.TrimPrefix(effect, "delete ")

	entities := []domain.Entity{}
	switch strings.ToLower(effect) {
	// TODO: set these 'attribute' effects via reflection
	case "target.ip":
		if pod, ok := source.(domain.Pod); ok {
			ips := []net.IPAddr{}
			res := results[0]
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
		res := results[0]
		list, err := k8s.ParsePodList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse PodList: %v", err))
		} else {
			for _, res := range list.Items {
				entities = append(entities, domain.NewPodFromK8sSpec(res))
			}
		}
	case "k8s.deploymentlist":
		res := results[0]
		list, err := k8s.ParseDeploymentList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse DeploymentList: %v", err))
		} else {
			for _, res := range list.Items {
				entities = append(entities, domain.NewDeploymentFromK8sSpec(res))
			}
		}
	case "k8s.serviceaccountlist":
		res := results[0]
		list, err := k8s.ParseServiceAccountList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse ServiceAccountList: %v", err))
		} else {
			for _, res := range list.Items {
				entities = append(entities, domain.NewServiceAccountFromK8sSpec(res))
			}
		}
	case "k8s.serviceaccount":
		name := args["Name"]
		ns := args["Namespace"]
		sa := domain.NewServiceAccount(name, ns)
		entities = append(entities, sa)
	case "k8s.pod":
		name := args["Name"]
		ns := args["Namespace"]
		pod := domain.NewPod(name, ns)
		entities = append(entities, pod)
	case "k8s.deployment":
		name := args["Name"]
		ns := args["Namespace"]
		pod := domain.NewDeployment(name, ns)
		entities = append(entities, pod)
	case "k8s.secretlist":
		res := results[0]
		secrets, err := ParseSecretList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse SecretList: %v", err))
		} else {
			for _, secret := range secrets {
				entities = append(entities, secret)
			}
		}
	case "k8s.nodelist":
		res := results[0]
		nodeList, err := k8s.ParseNodeList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Failed to parse NodeList: %v", err))
		} else {
			for _, node := range nodeList.Items {
				entities = append(entities, domain.NewK8sNodeFromK8sSpec(node))
			}
		}
	}

	newFacts := NewFacts{}
	removedFacts := RemovedFacts{}
	if isRemoveEffect {
		removedFacts = RemovedFacts{Entities: entities}
	} else {
		newFacts = NewFacts{Entities: entities}
	}

	return newFacts, removedFacts
}
