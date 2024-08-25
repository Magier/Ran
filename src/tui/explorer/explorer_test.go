package explorer

import "testing"

func TestAddEntry(t *testing.T) {
	base := newResourceGroup("base")
	groups := []string{"cluster", "namespace", "workload"}
	addEntry(&base, groups, "deployment")

	if len(base.subGroups) != 1 {
		t.Fatalf("expected 1 group, got %d", len(base.subGroups))
	}

	test := base.subGroups["cluster"].subGroups["namespace"].subGroups["workload"].resources[0] == "deployment"
	if !test {
		t.Fatalf("expected 'deployment' to be at desired subgroups")
	}

}
