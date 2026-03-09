package campaign

import (
	"testing"

	"github.com/Magier/Ran/domain"
)

func TestParseTargetIPsReceivedEffect(t *testing.T) {
	result := "::1 172.18.0.1"
	pod := domain.NewPod("test", "default")
	c := NewCampaign(nil)
	update, err := c.ParseEffect("sys.ip", pod, nil, nil, result)

	if err != nil {
		t.Fatalf("Failed to parse effect: %v", err)
	}

	if len(update.New.Entities) != 1 {
		t.Fatalf("Got more than 1 expected change when parsing effect")
	}

	e := update.New.Entities[0]
	if pod, ok := e.(domain.Pod); ok {
		if len(pod.IPs) != 2 {
			t.Fatalf("Expected 2 IPs as a result")
		}
	}
}
