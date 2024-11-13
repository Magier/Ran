package explorer

import (
	"testing"

	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
)

func TestAddEntity(t *testing.T) {
	// add pod
	entities := []domain.Entity{}

	c := campaign.NewCampaign()
	c.AddEntities(entities)
	model := NewExplorer(c, 0)

	model.rebuildEntries()

	// base := newResourceGroup("base")
	// groups := []string{"cluster", "namespace", "workload"}
	// addNode(&base, groups, "deployment")

	// if len(base.subGroups) != 1 {
	// 	t.Fatalf("expected 1 group, got %d", len(base.subGroups))
	// }

	// test := base.subGroups["cluster"].subGroups["namespace"].subGroups["workload"].resources[0] == "deployment"
	// if !test {
	// 	t.Fatalf("expected 'deployment' to be at desired subgroups")
	// }
}
