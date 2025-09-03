package campaign

import (
	"testing"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/domain"
)

func TestAddPodWhenItsWorkloadIsAlreadyKnownDoesNotGenerateExtraWorkload(t *testing.T) {
	a := armory.Armory{}
	c := NewCampaign(&a)
	ns := "default"
	name := "test-pod"
	wl := domain.NewDeployment(name, ns)
	c.AddEntities(wl)
	prevCount := len(c.GetEntities())

	p := domain.Pod{
		K8sEntity: domain.K8sEntity{
			Name:      name + "-123",
			Namespace: ns,
			Owner: domain.OwnerRef{
				Name: name,
				Kind: wl.GetKind(),
			},
		},
	}
	c.AddEntities(p)
	entities := c.GetEntities()
	if len(entities) != prevCount+1 {
		t.Errorf("Only the new pod should've been added")
	}
}
func TestUnpackResourceID(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		wantNS   string
		wantKind string
		wantName string
		wantErr  bool
	}{
		{
			name:     "Pod name only returns default namespace and pod kind",
			input:    "mypod",
			wantNS:   "default",
			wantKind: "Pod",
			wantName: "mypod",
			wantErr:  false,
		},
		{
			name:     "Full resource ID for pod returns correct values",
			input:    "ns/ns1/pod/mypod",
			wantNS:   "ns1",
			wantKind: "Pod",
			wantName: "mypod",
			wantErr:  false,
		},
		{
			name:     "Full resource ID for deployment returns correct values",
			input:    "ns/ns1/depl/mydeploy",
			wantNS:   "ns1",
			wantKind: "Deployment",
			wantName: "mydeploy",
			wantErr:  false,
		},
		{
			name:     "Full resource ID for ServiceAccount returns correct values",
			input:    "ns/ns1/sa/mydeploy",
			wantNS:   "ns1",
			wantKind: "ServiceAccount",
			wantName: "mydeploy",
			wantErr:  false,
		},
		{
			name:     "Invalid format with extra segments returns error",
			input:    "invalid/format/extra",
			wantNS:   "default",
			wantKind: "Pod",
			wantName: "invalid/format/extra",
			wantErr:  true,
		},
		{
			name:     "Namespace has only two parts",
			input:    "ns/my-ns",
			wantNS:   "my-ns",
			wantKind: "Namespace",
			wantName: "my-ns",
			wantErr:  false,
		},
		{
			name:     "Invalid ns format with missing segments returns error",
			input:    "ns/ns1/pod",
			wantNS:   "default",
			wantKind: "Pod",
			wantName: "ns/ns1/pod",
			wantErr:  true,
		},
		{
			name:     "Empty string returns error",
			input:    "",
			wantNS:   "",
			wantKind: "",
			wantName: "",
			wantErr:  true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			ns, kind, name, err := UnpackResourceID(tt.input)
			if tt.wantErr && err == nil {
				t.Errorf("UnpackResourceID(%q) expected error, got nil", tt.input)
			}
			if ns != tt.wantNS || kind != tt.wantKind || name != tt.wantName {
				t.Errorf("UnpackResourceID(%q) = (%q, %q, %q), want (%q, %q, %q)", tt.input, ns, kind, name, tt.wantNS, tt.wantKind, tt.wantName)
			}
			if !tt.wantErr && err != nil {
				t.Errorf("UnpackResourceID(%q) unexpected error: %v", tt.input, err)
			}
		})
	}
}
