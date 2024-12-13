package campaign

import (
	"testing"

	"github.com/Magier/Ran/domain"
)

func TestAddSinglePod(t *testing.T) {
	p := domain.NewPod("test", "default")

	c := NewCampaign()
	c.AddEntities(p)

	pods := c.GetPods()
	if len(pods) != 1 {
		t.Error("Expect exactly 1 pod in the knowledge base")
	}
}
