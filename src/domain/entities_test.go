package domain

import "testing"

func TestK8sEntityFromId(t *testing.T) {
	tests := []struct {
		id           string
		expectedName string
		expectedKind string
		expectedNS   string
	}{
		// For an id in the format "ns/test/pod/nginx",
		// the function splits the string and picks:
		{"ns/test/pod/nginx", "nginx", "Pod", "test"},
		// For a cluster-scoped resource with id "cr/nginx"
		// n = 2 gives: name = "nginx", kind = "ClusterRole" title-cased to "Pod",
		// and no namespace since n > 2 is false.
		{"cr/nginx", "nginx", "ClusterRole", ""},
	}

	for _, tt := range tests {
		entity := K8sEntityFromId(tt.id)
		if entity.GetName() != tt.expectedName {
			t.Errorf("For id %q, expected name %q, got %q", tt.id, tt.expectedName, entity.GetName())
		}
		if entity.GetKind() != tt.expectedKind {
			t.Errorf("For id %q, expected kind %q, got %q", tt.id, tt.expectedKind, entity.GetKind())
		}
		if entity.GetNamespace() != tt.expectedNS {
			t.Errorf("For id %q, expected namespace %q, got %q", tt.id, tt.expectedNS, entity.GetNamespace())
		}
	}
}
