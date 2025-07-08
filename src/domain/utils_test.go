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
			Id:        "pod/test",
			Name:      "test",
			Kind:      "Pod",
			Namespace: "default",
			Owner:     OwnerRef{},
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
func TestNormalizeResourceType(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{
			name:     "simple singular resource",
			input:    "pod",
			expected: "pods",
		},
		{
			name:     "already plural resource",
			input:    "pods",
			expected: "pods",
		},
		{
			name:     "resource ending with y",
			input:    "policy",
			expected: "policies",
		},
		{
			name:     "resource ending with y, vowel before y",
			input:    "key",
			expected: "keys",
		},
		{
			name:     "resource with subresource",
			input:    "pod/exec",
			expected: "pods/exec",
		},
		{
			name:     "resource with subresource ending with y",
			input:    "policy/status",
			expected: "policies/status",
		},
		{
			name:     "resource with subresource already plural",
			input:    "pods/logs",
			expected: "pods/logs",
		},
		{
			name:     "resource with uppercase letters",
			input:    "Deployment",
			expected: "deployments",
		},
		{
			name:     "resource with uppercase and subresource",
			input:    "Deployment/Status",
			expected: "deployments/status",
		},
		{
			name:     "empty string",
			input:    "",
			expected: "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := NormalizeResourceType(tt.input)
			if result != tt.expected {
				t.Errorf("NormalizeResourceType(%q) = %q; want %q", tt.input, result, tt.expected)
			}
		})
	}
}

func Test_mergeEntities_PodFieldsPreferNewValues(t *testing.T) {
	oldPod := NewPod("test", "default")
	oldPod.HostIPC = AsProbBool(false)
	newPod := NewPod("test", "default")
	newPod.HostIPC = AsProbBool(true)

	merged := mergeEntities(newPod, oldPod).(Pod)

	if merged.HostIPC != AsProbBool(true) {
		t.Errorf("Expected HostIPC to be from newPod, got %v", merged.HostIPC)
	}
}

func Test_mergeEntities_MergingSlicesDeduplicatesEntries(t *testing.T) {
	oldPod := NewPod("test", "default")
	oldPod.Mounts = []Mount{
		{
			Name:       "old-mount",
			MountPoint: "/old/path",
		},
	}
	newPod := NewPod("test", "default")
	newPod.Mounts = []Mount{
		{
			Name:       "new-mount",
			MountPoint: "/new/path",
		},
		{
			Name:       "old-mount",
			MountPoint: "/old/path",
		},
	}

	merged := mergeEntities(newPod, oldPod).(Pod)

	if len(merged.Mounts) != 2 {
		t.Errorf("Expected 2 mounts after merging, got %d", len(merged.Mounts))
	}
	if merged.Mounts[0].Name != "new-mount" || merged.Mounts[1].Name != "old-mount" {
		t.Errorf("Expected mounts to be 'new-mount' and 'old-mount', got %s and %s", merged.Mounts[0].Name, merged.Mounts[1].Name)
	}
	if merged.Mounts[0].MountPoint != "/new/path" || merged.Mounts[1].MountPoint != "/old/path" {
		t.Errorf("Expected mount paths to be '/new/path' and '/old/path', got %s and %s", merged.Mounts[0].MountPoint, merged.Mounts[1].MountPoint)
	}
}
