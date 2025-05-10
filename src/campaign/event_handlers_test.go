package campaign

import (
	"testing"

	"github.com/Magier/Ran/domain"
)

func TestParseTargetIPsReceivedEffect(t *testing.T) {
	result := "::1 172.18.0.1"
	pod := domain.NewPod("test", "default")
	new, _ := ParseEffect("target.ip", pod, result)

	if len(new.Entities) != 1 {
		t.Fatalf("Got more than 1 expected change when parsing effect")
	}

	e := new.Entities[0]
	if pod, ok := e.(domain.Pod); ok {
		if len(pod.IPs) != 2 {
			t.Fatalf("Expected 2 IPs as a result")
		}
	}
}
