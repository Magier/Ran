package campaign

import "testing"

func TestGetServicesFromEnvVar(t *testing.T) {
	vars := map[string]string{
		"KUBERNETES_PORT":               "tcp://10.96.0.1:443",
		"KUBERNETES_PORT_443_TCP":       "tcp://10.96.0.1:443",
		"KUBERNETES_PORT_443_TCP_ADDR":  "10.96.0.1",
		"KUBERNETES_PORT_443_TCP_PORT":  "443",
		"KUBERNETES_PORT_443_TCP_PROTO": "tcp",
		"KUBERNETES_SERVICE_HOST":       "10.96.0.1",
		"KUBERNETES_SERVICE_PORT":       "443",
		"KUBERNETES_SERVICE_PORT_HTTPS": "443",
		"TRACING_ENABLED":               "true",
		"TRIVY_PORT":                    "tcp://10.96.12.128:4954",
		"TRIVY_PORT_4954_TCP":           "tcp://10.96.12.128:4954",
		"TRIVY_PORT_4954_TCP_ADDR":      "10.96.12.128",
		"TRIVY_PORT_4954_TCP_PORT":      "4954",
		"TRIVY_PORT_4954_TCP_PROTO":     "tcp",
		"TRIVY_SERVICE_HOST":            "10.96.12.128",
		"TRIVY_SERVICE_PORT":            "4954",
		"TRIVY_SERVICE_PORT_TRIVY_HTTP": "4954",
		"MY_SERVER_PORT":                "tcp://10.96.180.142:8080",
		"MY_SERVER_PORT_8080_TCP":       "tcp://10.96.180.142:8080",
		"MY_SERVER_PORT_8080_TCP_ADDR":  "10.96.180.142",
		"MY_SERVER_PORT_8080_TCP_PORT":  "8080",
		"MY_SERVER_PORT_8080_TCP_PROTO": "tcp",
		"MY_SERVER_SERVICE_HOST":        "10.96.180.142",
		"MY_SERVER_SERVICE_PORT":        "8080",
		"MY_SERVER_SERVICE_PORT_GRPC":   "8080",
	}

	services := getServicesFromEnvVars(vars)

	if len(services) != 3 {
		t.Fail()
	}
}
