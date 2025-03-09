package mitre

import (
	"testing"
)

func TestGetTechniqueIDByName(t *testing.T) {
	// Save the original mapping and restore after the test.
	originalMapping := TechniqueNameMapping
	TechniqueNameMapping = map[string]string{
		"T1001":     "Example Technique",
		"T1059":     "Command and Scripting Interpreter",
		"T1059.004": "Command and Scripting Interpreter: Unix Shell",
		"T1059.006": "Python",
	}
	defer func() {
		TechniqueNameMapping = originalMapping
	}()

	tests := []struct {
		name          string
		input         string
		expectedID    string
		expectedFound bool
	}{
		{
			name:          "exact technique name match",
			input:         "Example Technique",
			expectedID:    "T1001",
			expectedFound: true,
		},
		{
			name:          "Technique, which also has subtequeniques",
			input:         "Command and Scripting Interpreter",
			expectedID:    "T1059",
			expectedFound: true,
		},
		{
			name:          "subtechnique fallback match",
			input:         "Command and Scripting Interpreter: Unix Shell",
			expectedID:    "T1059.004",
			expectedFound: true,
		},
		{
			name:          "subtechnique fallback match",
			input:         "Python",
			expectedID:    "T1059.006",
			expectedFound: true,
		},
		{
			name:          "technique not found",
			input:         "Nonexistent Technique",
			expectedID:    "",
			expectedFound: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			id, found := GetTechniqueIDByName(tt.input)
			if found != tt.expectedFound {
				t.Errorf("for input %q expected found %v, got %v", tt.input, tt.expectedFound, found)
			}
			if id != tt.expectedID {
				t.Errorf("for input %q expected ID %q, got %q", tt.input, tt.expectedID, id)
			}
		})
	}
}
func TestIsTechniqueID(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected bool
	}{
		{
			name:     "Valid technique ID without subtechnique",
			input:    "T1234",
			expected: true,
		},
		{
			name:     "Valid technique ID with subtechnique",
			input:    "T1234.567",
			expected: true,
		},
		{
			name:     "Too short technique ID",
			input:    "T1",
			expected: false,
		},
		{
			name:     "Technique ID missing digits",
			input:    "T",
			expected: false,
		},
		{
			name:     "Invalid starting character",
			input:    "X1234",
			expected: false,
		},
		{
			name:     "Dot at the end without digits",
			input:    "T1234.",
			expected: false,
		},
		{
			name:     "Non-digit character inside the number",
			input:    "T12A34",
			expected: false,
		},
		{
			name:     "Dot in wrong place",
			input:    "T.1234",
			expected: false,
		},
		{
			name:     "Letter immediately after 'T'",
			input:    "Ta234",
			expected: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := IsTechniqueID(tt.input)
			if result != tt.expected {
				t.Errorf("IsTechniqueID(%q) = %v; want %v", tt.input, result, tt.expected)
			}
		})
	}
}
