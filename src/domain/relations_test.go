package domain

import (
	"testing"

	"github.com/google/shlex"
)

func TestShellTokenization(t *testing.T) {
	tests := []struct {
		name   string
		input  string
		expect []string
	}{
		{
			name:   "simple command",
			input:  "ls -la /tmp",
			expect: []string{"ls", "-la", "/tmp"},
		},
		{
			name:   "multiple spaces",
			input:  "echo   hello    world",
			expect: []string{"echo", "hello", "world"},
		},
		{
			name:   "single quoted string",
			input:  `echo 'hello world'`,
			expect: []string{"echo", "hello world"},
		},
		{
			name:   "double quoted string",
			input:  `echo "hello world"`,
			expect: []string{"echo", "hello world"},
		},
		{
			name:  "curl with JSON data and headers",
			input: `curl -XPOST      https://kubernetes.default.svc.cluster.local/apis/authorization.k8s.io/v1/selfsubjectrulesreviews      --cacert /var/run/secrets/kubernetes.io/serviceaccount/ca.crt      -H "Authorization: Bearer "      -H "Content-Type: application/json"      --data '{ "kind": "SelfSubjectRulesReview", "apiVersion": "authorization.k8s.io/v1", "spec": { "namespace": "vuln-ingress-to-root" } }'`,
			expect: []string{
				"curl",
				"-XPOST",
				"https://kubernetes.default.svc.cluster.local/apis/authorization.k8s.io/v1/selfsubjectrulesreviews",
				"--cacert",
				"/var/run/secrets/kubernetes.io/serviceaccount/ca.crt",
				"-H",
				"Authorization: Bearer ",
				"-H",
				"Content-Type: application/json",
				"--data",
				`{ "kind": "SelfSubjectRulesReview", "apiVersion": "authorization.k8s.io/v1", "spec": { "namespace": "vuln-ingress-to-root" } }`,
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := shlex.Split(tt.input)
			if err != nil {
				t.Fatalf("shlex.Split error: %v", err)
			}
			if len(got) != len(tt.expect) {
				t.Fatalf("expected %d tokens, got %d: %v", len(tt.expect), len(got), got)
			}
			for i := range got {
				if got[i] != tt.expect[i] {
					t.Errorf("token[%d]: expected %q, got %q", i, tt.expect[i], got[i])
				}
			}
		})
	}
}
