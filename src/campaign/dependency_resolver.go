package campaign

import (
	"fmt"
	"regexp"
)

// DependencyResolver handles resolving map values with dependencies
type DependencyResolver struct {
	data     map[string]string
	resolved map[string]string
	visiting map[string]bool
	visited  map[string]bool
}

// NewDependencyResolver creates a new resolver
func NewDependencyResolver(data map[string]string) *DependencyResolver {
	return &DependencyResolver{
		data:     data,
		resolved: make(map[string]string),
		visiting: make(map[string]bool),
		visited:  make(map[string]bool),
	}
}

// extractReferences finds variable references in a value string
// Assumes references are in format ${key} or $key
func (dr *DependencyResolver) extractReferences(value string) []string {
	var refs []string

	// Match ${key} pattern
	re1 := regexp.MustCompile(`\$\{([^}]+)\}`)
	matches1 := re1.FindAllStringSubmatch(value, -1)
	for _, match := range matches1 {
		refs = append(refs, match[1])
	}

	// Match $key pattern (word boundaries)
	re2 := regexp.MustCompile(`\$([a-zA-Z_][a-zA-Z0-9_]*)`)
	matches2 := re2.FindAllStringSubmatch(value, -1)
	for _, match := range matches2 {
		// Avoid duplicates from ${key} pattern
		found := false
		for _, existing := range refs {
			if existing == match[1] {
				found = true
				break
			}
		}
		if !found {
			refs = append(refs, match[1])
		}
	}

	return refs
}

// resolve recursively resolves a key's value
// func (dr *DependencyResolver) resolve(key string) error {
// 	// Check for circular dependency
// 	if dr.visiting[key] {
// 		return fmt.Errorf("circular dependency detected involving key: %s", key)
// 	}

// 	// If already resolved, return
// 	if dr.visited[key] {
// 		return nil
// 	}

// 	// Check if key exists
// 	value, exists := dr.data[key]
// 	if !exists {
// 		return fmt.Errorf("key not found: %s", key)
// 	}

// 	// Mark as visiting
// 	dr.visiting[key] = true

// 	// Get dependencies
// 	refs := dr.extractReferences(value)

// 	// Resolve dependencies first
// 	for _, ref := range refs {
// 		if err := dr.resolve(ref); err != nil {
// 			return err
// 		}
// 	}

// 	// Now resolve current key
// 	resolvedValue := value
// 	for _, ref := range refs {
// 		// Replace references with resolved values
// 		refValue := dr.resolved[ref]
// 		resolvedValue = strings.ReplaceAll(resolvedValue, "${"+ref+"}", refValue)
// 		resolvedValue = strings.ReplaceAll(resolvedValue, "$"+ref, refValue)
// 	}

// 	// Store resolved value
// 	dr.resolved[key] = resolvedValue
// 	dr.visited[key] = true
// 	dr.visiting[key] = false

// 	return nil
// }

// ResolveAll resolves all values in the map
// func (dr *DependencyResolver) ResolveAll() (map[string]string, error) {
// 	for key := range dr.data {
// 		if err := dr.resolve(key); err != nil {
// 			return nil, err
// 		}
// 	}
// 	return dr.resolved, nil
// }

// GetEvaluationOrder returns the order in which keys should be evaluated
func (dr *DependencyResolver) GetEvaluationOrder() ([]string, error) {
	var order []string
	visited := make(map[string]bool)
	visiting := make(map[string]bool)

	var visit func(string) error
	visit = func(key string) error {
		if visiting[key] {
			return fmt.Errorf("circular dependency detected involving key: %s", key)
		}
		if visited[key] {
			return nil
		}

		visiting[key] = true

		// Visit dependencies first
		if value, exists := dr.data[key]; exists {
			refs := dr.extractReferences(value)
			for _, ref := range refs {
				if _, refExists := dr.data[ref]; refExists {
					// skip references to self, as these are propagated defaults
					if ref == key {
						continue
					} else if err := visit(ref); err != nil {
						return err
					}
				}
			}
		}

		visiting[key] = false
		visited[key] = true
		order = append(order, key)
		return nil
	}

	for key := range dr.data {
		if err := visit(key); err != nil {
			return nil, err
		}
	}

	return order, nil
}
