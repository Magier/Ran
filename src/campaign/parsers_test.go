package campaign

import (
	"testing"
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
