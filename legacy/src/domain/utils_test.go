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

func Test_mergeEntities_NewFilesAreMergedWithExisting(t *testing.T) {
	oldNode := NewK8sNode("node-1")
	oldNode.Files = []string{"/etc/passwd", "/etc/shadow"}

	newNode := NewK8sNode("node-1")
	newNode.Files = []string{"/etc/hosts", "/etc/passwd"}

	merged := mergeEntities(newNode, oldNode).(K8sNode)

	if len(merged.Files) != 3 {
		t.Fatalf("Expected 3 files after merging, got %d: %v", len(merged.Files), merged.Files)
	}
	expected := map[string]bool{"/etc/hosts": true, "/etc/passwd": true, "/etc/shadow": true}
	for _, f := range merged.Files {
		if !expected[f] {
			t.Errorf("Unexpected file in merged list: %s", f)
		}
	}
}

func Test_mergePodAndUnknownSystem(t *testing.T) {
	hostName := "pod-123"
	pod := NewPod(hostName, "default")
	pod.HostName = hostName

	os := "Linux"
	unknownSys := NewSystem(hostName, os, RootExec)
	unknownSys.EnvVars = map[string]string{
		"VAR": "Aarst",
	}

	resEntity := mergeEntities(unknownSys, pod)
	if resEntity == nil {
		t.Fatal("Expected merged entity to be non-nil")
	}

	merged, isPod := resEntity.(Pod)
	if !isPod {
		t.Fatalf("Expected merged entity to be Pod, got %T", resEntity)
	}

	if merged.HostName != hostName {
		t.Errorf("Expected HostName to be '%s', got %s", hostName, merged.HostName)
	}
	if merged.OS != os {
		t.Errorf("Expected OS to be '%s', got %s", os, merged.OS)
	}

	if merged.EnvVars["VAR"] != "Aarst" {
		t.Errorf("Expected EnvVars[VAR] to be 'Aarst', got %s", merged.EnvVars["VAR"])
	}
}

func Test_mergeSystems(t *testing.T) {
	type testCase struct {
		name     string
		a        System
		b        System
		wantType reflect.Type
		wantErr  bool
	}

	hostName := "pod-123"
	os := "Linux"
	pod := NewPod(hostName, "default")
	pod.HostName = hostName
	pod.OS = os

	// always set binaries on the first sys and envVars on 2nd; the final should have both
	binary := "curl"
	envVars := map[string]string{
		"VAR": "Aarst",
	}

	node := NewK8sNode(hostName)
	unknownSys := NewSystem(hostName, os, RootExec)
	tests := []testCase{
		{
			name:     "Pod vs Pod",
			a:        pod,
			b:        pod,
			wantType: reflect.TypeOf(pod),
		},
		{
			name:     "K8sNode vs K8sNode",
			a:        node,
			b:        node,
			wantType: reflect.TypeOf(node),
		},
		{
			name:     "Pod vs K8sNode",
			a:        pod,
			b:        node,
			wantType: nil,
			wantErr:  true,
		},
		{
			name:     "K8sNode vs Pod",
			a:        node,
			b:        pod,
			wantType: nil,
			wantErr:  true,
		},
		{
			name:     "Pod vs UnknownSystem (promotes to Pod)",
			a:        pod,
			b:        unknownSys,
			wantType: reflect.TypeOf(pod),
		},
		{
			name:     "UnknownSystem (promotes to Pod) vs Pod",
			a:        unknownSys,
			b:        pod,
			wantType: reflect.TypeOf(pod),
		},
		{
			name:     "K8sNode vs UnknownSystem (promotes to K8sNode)",
			a:        node,
			b:        unknownSys,
			wantType: reflect.TypeOf(node),
		},
		{
			name:     "UnknownSystem (promotes to K8sNode) vs K8sNode",
			a:        unknownSys,
			b:        node,
			wantType: reflect.TypeOf(node),
		},
		{
			name:     "Pod vs nil",
			a:        pod,
			b:        nil,
			wantType: reflect.TypeOf(pod),
		},
		{
			name:     "nil vs Pod",
			a:        nil,
			b:        pod,
			wantType: reflect.TypeOf(pod),
		},
		{
			name:     "K8sNode vs nil",
			a:        node,
			b:        nil,
			wantType: reflect.TypeOf(node),
		},
		{
			name:     "nil vs K8sNode",
			a:        nil,
			b:        node,
			wantType: reflect.TypeOf(node),
		},
		{
			name:     "UnknownSystem vs nil",
			a:        unknownSys,
			b:        nil,
			wantType: reflect.TypeOf(unknownSys),
		},
		{
			name:     "nil vs UnknownSystem",
			a:        nil,
			b:        unknownSys,
			wantType: reflect.TypeOf(unknownSys),
		},
		{
			name:    "nil vs nil",
			a:       nil,
			b:       nil,
			wantErr: true,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			if tc.a != nil {
				tc.a.SetEnvironmentVariables(envVars)
			}
			if tc.b != nil {
				tc.b.SetBinary(binary, binary)
			}

			got, err := mergeSystems(tc.a, tc.b)
			if err != nil {
				if !tc.wantErr {
					t.Errorf("mergeSystems returned error: %v", err)
				}
				return
			}
			if got != nil {

				if tc.wantType != nil && reflect.TypeOf(got) != tc.wantType {
					t.Errorf("Expected type %v, got %v", tc.wantType, reflect.TypeOf(got))
				}

				// fields set on A are correctly merged
				if !reflect.DeepEqual(got.GetEnvironmentVariables(), envVars) {
					t.Errorf("Expected environment variables %v, got %v", envVars, got.GetEnvironmentVariables())
				}

				// fields set on B are correctly merged
				if !got.HasBinary(binary).Bool() {
					t.Errorf("Expected binary %q to be set", binary)
				}
			}
		})
	}
}
