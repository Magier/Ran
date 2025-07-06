package campaign

import (
	"fmt"
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
	if len(removedFacts.Entities) != 0 {
		t.Errorf("Expected empty RemovedFacts, got %v", removedFacts)
	}
	if len(removedFacts.Relations) != 0 {
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
		Target: domain.Namespace{Name: nsName},
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
func TestAnalyzeFailedTTPExecution_ToolNotFound(t *testing.T) {
	toolName := "kubectl"
	target := domain.NewPod("mypod", "default")
	event := domain.TTPExecuted{
		Results: []string{fmt.Sprintf("command terminated with exit code 127: 'sh: 1: %s: not found\n'", toolName)},
		Procedure: domain.Procedure{
			Tool: toolName,
		},

		Target: target,
	}
	newFacts, _, err := analyzeFailedTTPExecution(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 1 {
		t.Errorf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	pod, ok := newFacts.Entities[0].(domain.Pod)
	if !ok {
		t.Fatalf("Expected entity to be Pod, got %T", newFacts.Entities[0])
	}
	if val, exists := pod.Binaries["kubectl"]; !exists || val != "❌" {
		t.Errorf("Expected pod.Binaries[kubectl]=❌, got %v", pod.Binaries)
	}
}

func TestAnalyzeFailedTTP_BinaryNotFoundShouldUpdateBinariesOnExecutingSystem(t *testing.T) {
	toolName := "curl"
	event := domain.TTPExecuted{
		Results: []string{
			"Error 127\n",
			"/usr/bin/sh: 1: curl: not found\n",
			"command terminated with exit code 127: '/usr/bin/sh: 1: curl: not found\n'"},
		Procedure: domain.Procedure{
			Tool: toolName,
		},
		Target: domain.NewPod("mypod", "default"),
	}

	newFacts, removedFacts, err := analyzeFailedTTPExecution(event)

	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 1 {
		t.Errorf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	pod, ok := newFacts.Entities[0].(domain.Pod)
	if !ok {
		t.Fatalf("Expected entity to be Pod, got %T", newFacts.Entities[0])
	}
	if val, exists := pod.Binaries[toolName]; !exists || val != "❌" {
		t.Errorf("Expected pod.Binaries[%s]=❌, got %v", toolName, pod.Binaries)
	}
	if len(removedFacts.Entities) != 0 {
		t.Errorf("Expected no removed entities, got %d", len(removedFacts.Entities))
	}
	if len(removedFacts.Relations) != 0 {
		t.Errorf("Expected no removed relations, got %d", len(removedFacts.Relations))
	}
}

func TestAnalyzeFailedTTPExecution_RBAC_ForbiddenWithUser(t *testing.T) {
	saName := "test-sa"
	ns := "test-ns"
	event := domain.TTPExecuted{
		Results: []string{
			fmt.Sprintf("command terminated with exit code 1: 'Error from server (Forbidden): pods is forbidden: User \"system:serviceaccount:%s:%s\" cannot list resource \"pods\" in API group \"\" in the namespace \"%s\"\n'", ns, saName, ns),
		},
		Procedure: domain.Procedure{Tool: "kubectl"},
		Target:    domain.NewPod("mypod", ns),
	}
	newFacts, _, err := analyzeFailedTTPExecution(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}

	// expect the returned entity is a service account
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	sa, ok := newFacts.Entities[0].(domain.ServiceAccount)
	if !ok {
		t.Fatalf("Expected entity to be ServiceAccount, got %T", newFacts.Entities[0])
	}
	if sa.Name != saName || sa.Namespace != ns {
		t.Errorf("Expected ServiceAccount name 'default' in namespace '%s', got name '%s' in namespace '%s'", ns, sa.Name, sa.Namespace)
	}

	if len(newFacts.Relations) != 0 {
		t.Errorf("Expected 0 relations, got %d", len(newFacts.Relations))
	}
}
