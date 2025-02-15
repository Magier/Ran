package campaign

import (
	"testing"

	"github.com/Magier/Ran/domain"
)

func TestParseTargetIPsReceivedEffect(t *testing.T) {
	result := "::1 172.18.0.1"
	pod := domain.NewPod("test", "default")
	msg := parseEffect("target.ip", pod, result)
	if msg == nil {
		t.Fatalf("Expected non-nil message, got nil")
	}

	ev, validEvent := msg.(domain.FactsChanged)
	if !validEvent {
		t.Fatalf("result from parseEffect is not a valid FactsChanged event")
	}

	if len(ev.NewEntities) != 1 {
		t.Fatalf("Got more than 1 expected change when parsing effect")
	}

	e := ev.NewEntities[0]
	if pod, ok := e.(domain.Pod); ok {
		if len(pod.IPs) != 2 {
			t.Fatalf("Expected 2 IPs as a result")
		}
	}
}
