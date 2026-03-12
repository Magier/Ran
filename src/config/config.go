package config

import (
	"log/slog"
	"os"

	"gopkg.in/yaml.v3"
)

// Config represents the Ran configuration
type Config struct {
	Namespaces NamespaceFilter `yaml:"namespaces"`
}

// NamespaceFilter controls which namespaces are visible in discovery
type NamespaceFilter struct {
	// Excluded contains namespaces to hide (blacklist mode)
	// If Included is empty and Excluded has items, only these namespaces are hidden
	Excluded []string `yaml:"excluded,omitempty"`

	// Included contains namespaces to show (whitelist mode)
	// If Included has items, only these namespaces are shown (takes precedence over Excluded)
	Included []string `yaml:"included,omitempty"`
}

var defaultConfig = Config{
	Namespaces: NamespaceFilter{
		Excluded: []string{"kube-system", "local-path-storage"},
	},
}

// Load reads configuration from the specified path, or defaults to ran.yaml in the current working directory
// If not found in current directory, checks parent directory as well
// If the file doesn't exist, returns the default configuration
func Load(path string) (*Config, error) {
	if path == "" {
		path = "ran.yaml"
	}

	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			// If default filename, try parent directory
			if path == "ran.yaml" {
				parentPath := "../ran.yaml"
				data, err = os.ReadFile(parentPath)
				if err != nil {
					if os.IsNotExist(err) {
						slog.Debug("Config file not found, using defaults", "paths", []string{path, parentPath})
						return &defaultConfig, nil
					}
					return nil, err
				}
				slog.Debug("Config loaded from parent directory", "path", parentPath)
			} else {
				// Specific path provided but doesn't exist - use defaults
				slog.Debug("Config file not found, using defaults", "path", path)
				return &defaultConfig, nil
			}
		} else {
			return nil, err
		}
	} else {
		slog.Debug("Config loaded", "path", path)
	}

	var cfg Config
	if err := yaml.Unmarshal(data, &cfg); err != nil {
		return nil, err
	}

	return &cfg, nil
}

// ShouldIncludeNamespace returns true if the namespace should be included based on the filter
// If Included is non-empty, it acts as a whitelist (only those namespaces are shown)
// Otherwise, Excluded acts as a blacklist (those namespaces are hidden)
func (nf *NamespaceFilter) ShouldIncludeNamespace(ns string) bool {
	// Whitelist mode: if Included is non-empty, only include if in the list
	if len(nf.Included) > 0 {
		for _, allowed := range nf.Included {
			if allowed == ns {
				return true
			}
		}
		return false
	}

	// Blacklist mode: exclude if in the Excluded list
	for _, excluded := range nf.Excluded {
		if excluded == ns {
			return false
		}
	}
	return true
}
