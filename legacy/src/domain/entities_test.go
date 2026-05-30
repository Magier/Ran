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
func TestRequirements_Satisfied(t *testing.T) {
	type args struct {
		target      Entity
		accessLevel AccessLevel
		state       State
	}
	tests := []struct {
		name   string
		req    Requirements
		args   args
		expect bool
	}{
		{
			name: "Abstract System mismatches Pod kind",
			req:  Requirements{Kind: "System"},
			args: args{
				target: K8sEntity{Name: "nginx", Kind: "Pod"},
				state:  State{},
			},
			expect: true,
		},
		{
			name: "Abstract System mismatches K8sNode kind",
			req:  Requirements{Kind: "System"},
			args: args{
				target: K8sEntity{Name: "k8s-worker", Kind: "Node"},
				state:  State{},
			},
			expect: true,
		},
		{
			name: "Kind matches, access level matches, no RBAC, no Exists",
			req:  Requirements{Kind: "Pod"},
			args: args{
				target: K8sEntity{Name: "nginx", Kind: "Pod"},
				state:  State{},
			},
			expect: true,
		},
		{
			name: "Kind mismatch returns false",
			req:  Requirements{Kind: "Service", AccessLevel: UserRead},
			args: args{
				target:      K8sEntity{Name: "nginx", Kind: "Pod"},
				accessLevel: UserRead,
				state:       State{},
			},
			expect: false,
		},
		{
			name: "AccessLevel not satisfied returns false",
			req:  Requirements{Kind: "Pod", AccessLevel: RootExec},
			args: args{
				target:      K8sEntity{Name: "nginx", Kind: "Pod"},
				accessLevel: UserRead,
				state:       State{},
			},
			expect: false,
		},
		{
			name: "RBACPermission required but not present returns false",
			req:  Requirements{Kind: "Pod", AccessLevel: UserRead, RBACPermissions: []RBACPermission{{Verb: "get", ResourceType: "pods"}}},
			args: args{
				target:      K8sEntity{Name: "nginx", Kind: "Pod"},
				accessLevel: UserRead,
				state:       State{Entitlements: map[string][]string{}},
			},
			expect: false,
		},
		{
			name: "RBACPermission required and wildcard present returns true",
			req:  Requirements{Kind: "Pod", AccessLevel: UserRead, RBACPermissions: []RBACPermission{{Verb: "get", ResourceType: "pods"}}},
			args: args{
				target:      K8sEntity{Name: "nginx", Kind: "Pod"},
				accessLevel: UserRead,
				state:       State{Entitlements: map[string][]string{"* *": {"admin"}}},
			},
			expect: true,
		},
		{
			name: "EntitiesExists required and state satisfies returns true",
			req:  Requirements{Kind: "Pod", AccessLevel: UserRead, Exists: EntitiesExists{"pod"}},
			args: args{
				target:      K8sEntity{Name: "nginx", Kind: "Pod"},
				accessLevel: UserRead,
				state:       State{EntityCounts: map[string]int{"pod": 1}},
			},
			expect: true,
		},
		{
			name: "EntitiesExists required and state does not satisfy returns false",
			req:  Requirements{Kind: "Pod", AccessLevel: UserRead, Exists: EntitiesExists{"pod"}},
			args: args{
				target:      K8sEntity{Name: "nginx", Kind: "Pod"},
				accessLevel: UserRead,
				state:       State{EntityCounts: map[string]int{"pod": 0}},
			},
			expect: false,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.req.Satisfied(tt.args.target, tt.args.accessLevel, tt.args.state)
			if got != tt.expect {
				t.Errorf("Requirements.Satisfied() = %v, want %v", got, tt.expect)
			}
		})
	}
}
