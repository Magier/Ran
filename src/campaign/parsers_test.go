package campaign

import (
	"testing"

	"github.com/Magier/Ran/domain"
)

func TestSelfSubjectReviewResult_ForbiddenStatus(t *testing.T) {
	res := `{
		"kind": "Status",
		"apiVersion": "v1",
		"metadata": {},
		"status": "Failure",
		"message": "selfsubjectrulesreviews.authorization.k8s.io is forbidden: User \"system:anonymous\" cannot create resource \"selfsubjectrulesreviews\" in API group \"authorization.k8s.io\" at the cluster scope",
		"reason": "Forbidden",
		"details": {
		  "group": "authorization.k8s.io",
		  "kind": "selfsubjectrulesreviews"
		},
		"code": 403
	}`
	var _ = res

	// Genereated stuff:
	// ev := domain.TTPExecuted{}
	// source := domain.Entity{}
	// args := []string{"", "Forbidden"}

	// result, err := HandleSelfSubjectReviewResult(ev, source, args...)
	// if err != nil {
	// 	t.Fatalf("Expected no error, got: %v", err)
	// }

	// expected := domain.SelfSubjectReviewResult{
	// 	Source: source,
	// 	Status: "Forbidden",
	// }

	//	if result != expected {
	//		t.Fatalf("Expected %v, got: %v", expected, result)
	//	}
}
func TestParseEffect_TargetIP(t *testing.T) {
	source := domain.NewPod("mypod", "myns")
	args := map[string]string{}
	results := []string{"10.0.0.1 10.0.0.2"}
	newFacts, removedFacts := ParseEffect("target.ip", source, args, results...)
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	pod, ok := newFacts.Entities[0].(domain.Pod)
	if !ok {
		t.Fatalf("Expected entity to be Pod")
	}
	if len(pod.IPs) != 2 {
		t.Fatalf("Expected 2 IPs, got %d", len(pod.IPs))
	}
	if pod.IPs[0].IP.String() != "10.0.0.1" || pod.IPs[1].IP.String() != "10.0.0.2" {
		t.Fatalf("Unexpected IPs: %+v", pod.IPs)
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_K8sPodList(t *testing.T) {
	// minimal pod list json
	results := []string{`{"items":[{"metadata":{"name":"pod1","namespace":"ns1"}},{"metadata":{"name":"pod2","namespace":"ns2"}}]}`}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("k8s.podlist", source, args, results...)
	if len(newFacts.Entities) != 2 {
		t.Fatalf("Expected 2 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_K8sDeploymentList(t *testing.T) {
	results := []string{`{"items":[{"metadata":{"name":"dep1","namespace":"ns1"}},{"metadata":{"name":"dep2","namespace":"ns2"}}]}`}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("k8s.deploymentlist", source, args, results...)
	if len(newFacts.Entities) != 2 {
		t.Fatalf("Expected 2 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_K8sServiceAccountList(t *testing.T) {
	results := []string{`{"items":[{"metadata":{"name":"sa1","namespace":"ns1"}},{"metadata":{"name":"sa2","namespace":"ns2"}}]}`}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("k8s.serviceaccountlist", source, args, results...)
	if len(newFacts.Entities) != 2 {
		t.Fatalf("Expected 2 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_K8sServiceAccount_Created(t *testing.T) {
	results := []string{"serviceaccount/my-sa created"}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{"Name": "my-sa", "Namespace": "ns1"}
	newFacts, removedFacts := ParseEffect("k8s.serviceaccount", source, args, results...)
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	sa, ok := newFacts.Entities[0].(domain.ServiceAccount)
	if !ok {
		t.Fatalf("Expected entity to be ServiceAccount")
	}
	if sa.GetName() != "my-sa" || sa.GetNamespace() != "ns1" {
		t.Fatalf("Unexpected ServiceAccount: %+v", sa)
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_K8sServiceAccount_AlreadyExists(t *testing.T) {
	results := []string{"Error: already exists"}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{"Name": "my-sa", "Namespace": "ns1"}
	newFacts, removedFacts := ParseEffect("k8s.serviceaccount", source, args, results...)
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	sa, ok := newFacts.Entities[0].(domain.ServiceAccount)
	if !ok {
		t.Fatalf("Expected entity to be ServiceAccount")
	}
	if sa.GetName() != "my-sa" || sa.GetNamespace() != "ns1" {
		t.Fatalf("Unexpected ServiceAccount: %+v", sa)
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_DeleteK8sServiceAccount(t *testing.T) {
	results := []string{"serviceaccount/my-sa deleted"}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{"Name": "my-sa", "Namespace": "ns1"}
	newFacts, removedFacts := ParseEffect("delete k8s.serviceaccount", source, args, results...)
	if len(removedFacts.Entities) != 1 {
		t.Fatalf("Expected 1 removed entity, got %d", len(removedFacts.Entities))
	}
	sa, ok := removedFacts.Entities[0].(domain.ServiceAccount)
	if !ok {
		t.Fatalf("Expected removed entity to be ServiceAccount")
	}
	if sa.GetName() != "my-sa" || sa.GetNamespace() != "ns1" {
		t.Fatalf("Unexpected ServiceAccount: %+v", sa)
	}
	if len(newFacts.Entities) != 0 {
		t.Fatalf("Expected 0 new entities")
	}
}

func TestParseEffect_DeleteK8sPod(t *testing.T) {
	results := []string{"irrelevant"}
	ns := "test-ns"
	name := "mypod"
	source := domain.NewPod(name, ns)
	args := map[string]string{"Name": name, "Namespace": ns}
	newFacts, removedFacts := ParseEffect("delete k8s.pod", source, args, results...)
	if len(removedFacts.Entities) != 1 {
		t.Fatalf("Expected 1 removed entity, got %d", len(removedFacts.Entities))
	}
	pod, ok := removedFacts.Entities[0].(domain.Pod)
	if !ok {
		t.Fatalf("Expected removed entity to be Pod")
	}
	if pod.GetName() != name || pod.GetNamespace() != ns {
		t.Fatalf("Unexpected Pod: %+v", pod)
	}
	if len(newFacts.Entities) != 0 {
		t.Fatalf("Expected 0 new entities")
	}
}

func TestParseEffect_DeleteK8sDeployment(t *testing.T) {
	results := []string{"irrelevant"}
	ns := "test-ns"
	name := "mydeployment"
	source := domain.NewDeployment(name, ns)
	args := map[string]string{"Name": name, "Namespace": ns}
	newFacts, removedFacts := ParseEffect("delete k8s.deployment", source, args, results...)
	if len(removedFacts.Entities) != 1 {
		t.Fatalf("Expected 1 removed entity, got %d", len(removedFacts.Entities))
	}
	dep, ok := removedFacts.Entities[0].(domain.Deployment)
	if !ok {
		t.Fatalf("Expected removed entity to be Deployment")
	}
	if dep.GetName() != name || dep.GetNamespace() != ns {
		t.Fatalf("Unexpected Deployment: %+v", dep)
	}
	if len(newFacts.Entities) != 0 {
		t.Fatalf("Expected 0 new entities")
	}
}

func TestParseEffect_K8sSecretList(t *testing.T) {
	results := []string{`{"items":[{"metadata":{"name":"secret1","namespace":"ns1"}},{"metadata":{"name":"secret2","namespace":"ns2"}}]}`}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("k8s.secretlist", source, args, results...)
	if len(newFacts.Entities) != 2 {
		t.Fatalf("Expected 2 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_UnknownEffect(t *testing.T) {
	results := []string{"irrelevant"}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("unknown.effect", source, args, results...)
	if len(newFacts.Entities) != 0 {
		t.Fatalf("Expected 0 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_NoResults(t *testing.T) {
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("k8s.podlist", source, args)
	if len(newFacts.Entities) != 0 {
		t.Fatalf("Expected 0 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}
