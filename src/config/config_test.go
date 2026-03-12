package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestShouldIncludeNamespace_ExcludeMode(t *testing.T) {
	filter := NamespaceFilter{
		Excluded: []string{"kube-system", "local-path-storage"},
	}

	tests := []struct {
		namespace string
		expected  bool
	}{
		{"default", true},
		{"kube-system", false},
		{"local-path-storage", false},
		{"production", true},
		{"kube-public", true},
	}

	for _, tt := range tests {
		result := filter.ShouldIncludeNamespace(tt.namespace)
		if result != tt.expected {
			t.Errorf("ExcludeMode: namespace %s: expected %v, got %v", tt.namespace, tt.expected, result)
		}
	}
}

func TestShouldIncludeNamespace_IncludeOnlyMode(t *testing.T) {
	filter := NamespaceFilter{
		Included: []string{"default", "production", "staging"},
	}

	tests := []struct {
		namespace string
		expected  bool
	}{
		{"default", true},
		{"production", true},
		{"staging", true},
		{"kube-system", false},
		{"development", false},
	}

	for _, tt := range tests {
		result := filter.ShouldIncludeNamespace(tt.namespace)
		if result != tt.expected {
			t.Errorf("IncludeOnlyMode: namespace %s: expected %v, got %v", tt.namespace, tt.expected, result)
		}
	}
}

func TestLoad_DefaultConfig(t *testing.T) {
	// Load from a non-existent path should return default config
	cfg, err := Load("/nonexistent/path/config.yaml")
	if err != nil {
		t.Fatalf("Expected no error for missing config, got: %v", err)
	}

	if len(cfg.Namespaces.Excluded) != 2 {
		t.Errorf("Expected 2 default excluded namespaces, got: %d", len(cfg.Namespaces.Excluded))
	}
}

func TestLoad_ValidConfig(t *testing.T) {
	// Create a temporary config file
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "config.yaml")

	configContent := `namespaces:
  included:
    - default
    - production
`
	err := os.WriteFile(configPath, []byte(configContent), 0644)
	if err != nil {
		t.Fatalf("Failed to create temp config: %v", err)
	}

	cfg, err := Load(configPath)
	if err != nil {
		t.Fatalf("Failed to load config: %v", err)
	}

	if len(cfg.Namespaces.Included) != 2 {
		t.Errorf("Expected 2 included namespaces, got: %d", len(cfg.Namespaces.Included))
	}

	if cfg.Namespaces.Included[0] != "default" {
		t.Errorf("Expected first namespace 'default', got: %s", cfg.Namespaces.Included[0])
	}
}

func TestLoad_BothIncludedAndExcluded(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "config.yaml")

	configContent := `namespaces:
  included:
    - default
  excluded:
    - kube-system
`
	err := os.WriteFile(configPath, []byte(configContent), 0644)
	if err != nil {
		t.Fatalf("Failed to create temp config: %v", err)
	}

	cfg, err := Load(configPath)
	if err != nil {
		t.Fatalf("Failed to load config: %v", err)
	}

	// When both are specified, Included takes precedence (whitelist mode)
	// Should only include "default"
	if !cfg.Namespaces.ShouldIncludeNamespace("default") {
		t.Error("Expected 'default' to be included (in whitelist)")
	}
	if cfg.Namespaces.ShouldIncludeNamespace("kube-system") {
		t.Error("Expected 'kube-system' to be excluded (not in whitelist)")
	}
	if cfg.Namespaces.ShouldIncludeNamespace("production") {
		t.Error("Expected 'production' to be excluded (not in whitelist)")
	}
}
