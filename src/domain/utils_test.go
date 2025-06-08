package domain

import (
	"reflect"
	"testing"
)

func TestCleanEventName(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{
			name:     "strip domain prefix and add dash",
			input:    "domain.FooBar",
			expected: "foo-bar",
		},
		{
			name:     "multiple uppercase letters",
			input:    "domain.HTTPResponse",
			expected: "http-response",
		},
		{
			name:     "no domain prefix",
			input:    "HelloWorld",
			expected: "hello-world",
		},
		{
			name:     "already lowercase",
			input:    "domain.hello",
			expected: "hello",
		},
		{
			name:     "package is used as prefix",
			input:    "package.IsLoaded",
			expected: "package-is-loaded",
		},
		{
			name:     "empty string",
			input:    "",
			expected: "",
		},
		{
			name:     "only domain prefix",
			input:    "domain.",
			expected: "",
		},
		{
			name:     "mixed case without prefix",
			input:    "testCaseExample",
			expected: "test-case-example",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := CleanEventName(tt.input)
			if result != tt.expected {
				t.Errorf("CleanEventName(%q) = %q; want %q", tt.input, result, tt.expected)
			}
		})
	}
}

func TestUpdateEntity_MergesFieldsCorrectly(t *testing.T) {
	newPod := NewPod("test", "default")
	newPod.HostIPC = AsProbBool(true)

	oldPod := NewPod("test", "default")
	oldPod.AccessLevel = UserExec

	// Should copy AccessLevel from oldPod, because the new one has no additional information
	merged := UpdateEntity(newPod, oldPod).(Pod)
	if merged.AccessLevel != UserExec {
		t.Errorf("Expected AccessLevel to be 5, got %d", merged.AccessLevel)
	}
	if merged.HostIPC != AsProbBool(true) {
		t.Errorf("Expected HostIPC to be true, got %v", merged.HostIPC)
	}
}

func TestUpdateEntity_DeepEqualReturnsOriginal(t *testing.T) {
	pod := Pod{
		K8sEntity: K8sEntity{
			Id:          "pod/test",
			Name:        "test",
			Kind:        "Pod",
			Namespace:   "default",
			AccessLevel: UserExec,
			Owner:       OwnerRef{},
		},
	}
	result := UpdateEntity(pod, pod)
	if !reflect.DeepEqual(result, Entity(pod)) {
		t.Error("Expected updateEntity to return the original entity when DeepEqual")
	}
}

func TestUpdateEntity_OwnableMergesOwner(t *testing.T) {
	// OwnerRef is empty in entity, but set in other
	entity := Pod{
		K8sEntity: K8sEntity{
			Id:        "pod/test",
			Name:      "test",
			Kind:      "Pod",
			Namespace: "default",
			Owner:     OwnerRef{},
		},
	}
	other := Pod{
		K8sEntity: K8sEntity{
			Id:        "pod/test",
			Name:      "test",
			Kind:      "Pod",
			Namespace: "default",
			Owner: OwnerRef{
				Kind: "Deployment",
				Name: "deploy2",
			},
		},
	}
	merged := UpdateEntity(entity, other).(Pod)
	if merged.Owner.Name != "deploy2" || merged.Owner.Kind != "Deployment" {
		t.Errorf("Expected Owner to be merged from other, got %+v", merged.Owner)
	}
}
