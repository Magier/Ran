package domain

import "testing"

func TestK8sEntityFromId(t *testing.T) {
	tests := []struct {
		id           string
		expectedName string
		expectedKind string
		expectedNS   string
	}{
		// For an id in the format "ns/default/pod/nginx",
		// the function splits the string and picks:
		// name = last component ("nginx"),
		// kind = second-to-last ("pod") then title-cased to "Pod",
		// namespace = element at index n-4, which for n=4 is the first element.
		{"ns/default/pod/nginx", "nginx", "Pod", "ns"},
		// For a cluster-scoped resource with id "pod/nginx"
		// n = 2 gives: name = "nginx", kind = "pod" title-cased to "Pod",
		// and no namespace since n > 2 is false.
		{"pod/nginx", "nginx", "Pod", ""},
		// Arbitrary id: "a/b/c/d/e" splits into ["a", "b", "c", "d", "e"]:
		// name = "e", kind = "d" becomes "D", namespace = element at index 1 ("b")
		{"a/b/c/d/e", "e", "D", "b"},
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
