package domain

import "testing"

func TestCleanEventName(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{
			name:     "strip domain prefix and add dash",
			input:    "domain.FooBar",
			expected: "foo-bar",
		},
		{
			name:     "multiple uppercase letters",
			input:    "domain.HTTPResponse",
			expected: "http-response",
		},
		{
			name:     "no domain prefix",
			input:    "HelloWorld",
			expected: "hello-world",
		},
		{
			name:     "already lowercase",
			input:    "domain.hello",
			expected: "hello",
		},
		{
			name:     "package is used as prefix",
			input:    "package.IsLoaded",
			expected: "package-is-loaded",
		},
		{
			name:     "empty string",
			input:    "",
			expected: "",
		},
		{
			name:     "only domain prefix",
			input:    "domain.",
			expected: "",
		},
		{
			name:     "mixed case without prefix",
			input:    "testCaseExample",
			expected: "test-case-example",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := CleanEventName(tt.input)
			if result != tt.expected {
				t.Errorf("CleanEventName(%q) = %q; want %q", tt.input, result, tt.expected)
			}
		})
	}
}
