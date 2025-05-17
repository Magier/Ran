package campaign

import (
	"testing"

	"github.com/Magier/Ran/domain"
)

func TestGetServicesFromEnvVar(t *testing.T) {
	vars := map[string]string{
		"KUBERNETES_PORT":               "tcp://10.96.0.1:443",
		"KUBERNETES_PORT_443_TCP":       "tcp://10.96.0.1:443",
		"KUBERNETES_PORT_443_TCP_ADDR":  "10.96.0.1",
		"KUBERNETES_PORT_443_TCP_PORT":  "443",
		"KUBERNETES_PORT_443_TCP_PROTO": "tcp",
		"KUBERNETES_SERVICE_HOST":       "10.96.0.1",
		"KUBERNETES_SERVICE_PORT":       "443",
		"KUBERNETES_SERVICE_PORT_HTTPS": "443",
		"TRACING_ENABLED":               "true",
		"TRIVY_PORT":                    "tcp://10.96.12.128:4954",
		"TRIVY_PORT_4954_TCP":           "tcp://10.96.12.128:4954",
		"TRIVY_PORT_4954_TCP_ADDR":      "10.96.12.128",
		"TRIVY_PORT_4954_TCP_PORT":      "4954",
		"TRIVY_PORT_4954_TCP_PROTO":     "tcp",
		"TRIVY_SERVICE_HOST":            "10.96.12.128",
		"TRIVY_SERVICE_PORT":            "4954",
		"TRIVY_SERVICE_PORT_TRIVY_HTTP": "4954",
		"MY_SERVER_PORT":                "tcp://10.96.180.142:8080",
		"MY_SERVER_PORT_8080_TCP":       "tcp://10.96.180.142:8080",
		"MY_SERVER_PORT_8080_TCP_ADDR":  "10.96.180.142",
		"MY_SERVER_PORT_8080_TCP_PORT":  "8080",
		"MY_SERVER_PORT_8080_TCP_PROTO": "tcp",
		"MY_SERVER_SERVICE_HOST":        "10.96.180.142",
		"MY_SERVER_SERVICE_PORT":        "8080",
		"MY_SERVER_SERVICE_PORT_GRPC":   "8080",
	}

	services := getServicesFromEnvVars(vars)

	if len(services) != 3 {
		t.Fail()
	}
}
func TestAnalyzeDeployPodFailure_NoResults(t *testing.T) {
	event := domain.TTPExecuted{
		Results: []string{},
	}
	newFacts, removedFacts, err := analyzeDeployPodFailure(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 0 {
		t.Errorf("Expected no entities, got %v", newFacts.Entities)
	}
	if len(newFacts.Relations) != 0 {
		t.Errorf("Expected no relations, got %v", newFacts.Relations)
	}
	if len(removedFacts.Entities) == 0 {
		t.Errorf("Expected empty RemovedFacts, got %v", removedFacts)
	}
	if len(removedFacts.Relations) == 0 {
		t.Errorf("Expected empty RemovedFacts, got %v", removedFacts)
	}
}

func TestAnalyzeDeployPodFailure_AlreadyExists(t *testing.T) {
	event := domain.TTPExecuted{
		Results: []string{"Error from server (AlreadyExists): pods \"mypod\" already exists"},
	}
	newFacts, _, err := analyzeDeployPodFailure(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 0 {
		t.Errorf("Expected no entities, got %v", newFacts.Entities)
	}
}

func TestAnalyzeDeployPodFailure_PodSecurityViolation(t *testing.T) {
	nsName := "test-ns"
	event := domain.TTPExecuted{
		Results: []string{
			"command terminated with exit code 1: 'Error from server (Forbidden): error when creating \"STDIN\": pods \"workstation-66549c6f86-vgqch-44183\" is forbidden: violates PodSecurity \"baseline:latest\": hostPath volumes (volume \"hostmount\")\n'",
		},
		Target: &domain.Namespace{Name: nsName},
	}
	newFacts, _, err := analyzeDeployPodFailure(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	ns, ok := newFacts.Entities[0].(domain.Namespace)
	if !ok {
		t.Fatalf("Expected entity to be Namespace, got %T", newFacts.Entities[0])
	}
	if ns.Name != nsName {
		t.Errorf("Expected namespace name '%s', got %s", nsName, ns.Name)
	}
	if ns.EnforcedPSS != "baseline:latest" {
		t.Errorf("Expected EnforcedPSS 'baseline:latest', got %s", ns.EnforcedPSS)
	}
}

func TestAnalyzeDeployPodFailure_PodSecurityViolation_without_target_returns_ns(t *testing.T) {
	nsName := "test-ns"
	event := domain.TTPExecuted{
		Args: map[string]string{
			"Namespace": nsName,
		},
		Results: []string{
			"command terminated with exit code 1: 'Error from server (Forbidden): error when creating \"STDIN\": pods \"workstation-66549c6f86-vgqch-44183\" is forbidden: violates PodSecurity \"baseline:latest\": hostPath volumes (volume \"hostmount\")\n'",
		},
	}
	newFacts, _, err := analyzeDeployPodFailure(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	ns, ok := newFacts.Entities[0].(domain.Namespace)
	if !ok {
		t.Fatalf("Expected entity to be Namespace, got %T", newFacts.Entities[0])
	}
	if ns.Name != "test-ns" {
		t.Errorf("Expected namespace name '%s', got %s", nsName, ns.Name)
	}
	if ns.EnforcedPSS != "baseline:latest" {
		t.Errorf("Expected EnforcedPSS 'baseline:latest', got %s", ns.EnforcedPSS)
	}
}

func TestAnalyzeDeployPodFailure_UnknownError(t *testing.T) {
	event := domain.TTPExecuted{
		Results: []string{"Some unknown error"},
	}
	newFacts, _, err := analyzeDeployPodFailure(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 0 {
		t.Errorf("Expected no entities, got %v", newFacts.Entities)
	}
}
