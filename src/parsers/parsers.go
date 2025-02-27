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

func HandleEnvVarResult(source domain.Entity, args ...any) (domain.Event, error) {
	if len(args) == 0 {
		return nil, errors.New("No environment variables received!")
	}
	stderr := args[1].(string)
	if stderr != "" {
		return nil, errors.New(stderr)
	}

	stdout := args[0].(string)
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

func HandleSaTokenRead(source domain.Entity, args ...any) (domain.Event, error) {
	if len(args) == 0 {
		return nil, fmt.Errorf("No SA token provided as argument")
	}

	var token string
	switch t := args[0].(type) {
	case string:
		token = t
	case []byte:
		token = string(t)
	}
	if len(token) == 0 {
		return nil, fmt.Errorf("Empty SA token can't be decoded")
	}
	if len(args) > 1 {
		if args[1] != "" {
			return nil, fmt.Errorf("Sa Token Read expects exactly 1 argument - received %d", len(args))
		}
	}
	return domain.ServiceAccountTokenExtracted{
		SourceSystemId: source.GetId(),
		Token:          token,
	}, nil
}

func HandleSelfSubjectReviewResult(source domain.Entity, args ...any) (domain.Event, error) {
	// try parse JSON
	if len(args) == 0 {
		return nil, fmt.Errorf("No data")
	}
	jsonData, ok := args[0].(string)
	if !ok {
		return nil, fmt.Errorf("Expected string data")
	}

	var result k8s_types.SelfSubjectRulesReview
	err := json.Unmarshal([]byte(jsonData), &result)
	if err != nil {
		return nil, fmt.Errorf("Failed to unmarshal JSON: %w", err)
	}

	if result.Code >= 400 {
		return domain.TTPFailed{
			Reason: result.Message,
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

func HandleNewContainer(source domain.Entity, args ...any) (domain.Event, error) {
	numArgs := len(args)
	if numArgs == 0 {
		return nil, fmt.Errorf("No data")
	}
	if numArgs != 3 {
		return nil, fmt.Errorf("Expected podName, namespaceName and podConfig; got %d args instead", numArgs)
	}

	podName := args[0].(string)
	nsName := args[1].(string)
	ns := domain.Namespace{Name: nsName}
	p := domain.NewPod(podName, nsName)
	cfg := args[2].(domain.PodConfig)

	p.HostIPC = domain.NewProbBool(cfg.HostIPC)
	p.HostPID = domain.NewProbBool(cfg.HostPID)
	p.HostNetwork = domain.NewProbBool(cfg.HostNetwork)
	p.Privileged = domain.NewProbBool(cfg.Privileged)

	// rels := []domain.Relation{
	// 	domain.Contains{Container: ns, Object: p},
	//  }
	return domain.FactsChanged{
		NewEntities: []domain.Entity{ns, p},
		// NewRelations: rels,
	}, nil
}
