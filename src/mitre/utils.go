package mitre

import (
	"strings"
	"unicode"
)

//go:generate go run gen_mappings.go https://raw.githubusercontent.com/mitre-attack/attack-stix-data/master/enterprise-attack/enterprise-attack.json

func IsTechniqueID(id string) bool {
	// A basic MITRE technique ID starts with "T" followed by digits.
	// It can also include an optional dot and extra digits (e.g., T1234.005).
	if len(id) < 5 || id[0] != 'T' {
		return false
	}

	// Consume digits immediately after 'T'.
	i := 1
	for i < len(id) && unicode.IsDigit(rune(id[i])) {
		i++
	}
	if i == 1 {
		return false // No digits found.
	}

	// If there is a dot, consume following digits.
	if i < len(id) {
		if id[i] == '.' {
			i++
			if i == len(id) {
				return false // Dot cannot be last character.
			}
			for ; i < len(id); i++ {
				if !unicode.IsDigit(rune(id[i])) {
					return false
				}
			}
		} else {
			return false // Unexpected character encountered.
		}
	}

	return true
}

func GetTechniqueIDByName(name string) (string, bool) {
	names := []string{name}

	// fallback: check if the name is a subtechnique which is just an addition to the technique name
	// e.g. "Command and Scripting Interpreter: Unix Shell" -> "Unix Shell"
	if parts := strings.SplitN(name, ":", 2); len(parts) == 2 { // it's a subtechnique
		subName := strings.TrimSpace(parts[1])
		names = append(names, subName)
	}

	for id, techniqueName := range TechniqueNameMapping {
		for _, n := range names {
			if techniqueName == n {
				return id, true
			}
		}
	}

	return "", false
}
