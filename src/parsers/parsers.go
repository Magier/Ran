package parsers

import (
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"

	"github.com/Magier/Ran/domain"
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
	}
	return nil
}

func HandleEnvVarResult(source domain.Entity, args ...string) (domain.Event, error) {
	if len(args) == 0 {
		return nil, errors.New("No environment variables received!")
	}
	stderr := args[1]
	if stderr != "" {
		return nil, errors.New(stderr)
	}

	stdout := args[0]
	vars := make(map[string]string)
	for _, l := range strings.Split(stdout, "\n") {
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

func HandleSaTokenRead(source domain.Entity, args ...string) (domain.Event, error) {
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

func HandleSelfSubjectReviewResult(source domain.Entity, args ...string) (domain.Event, error) {
	// try parse JSON
	if len(args) == 0 {
		return nil, fmt.Errorf("No data")
	}
	jsonData := args[0]
	var result k8s_types.SelfSubjectRulesReview
	err := json.Unmarshal([]byte(jsonData), &result)
	if err != nil {
		return nil, fmt.Errorf("Failed to unmarshal JSON: %w", err)
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

func HandleNewContainer(source domain.Entity, args ...string) (domain.Event, error) {
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
