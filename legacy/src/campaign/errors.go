package campaign

import "fmt"

// NotFoundError represents an error when a requested resource cannot be found
type NotFoundError struct {
	ResourceType string // "TTP", "Target", "Procedure", etc.
	ResourceID   string
}

func (e *NotFoundError) Error() string {
	return fmt.Sprintf("%s with ID '%s' not found", e.ResourceType, e.ResourceID)
}

// NewTTPNotFoundError creates a NotFoundError for a TTP
func NewTTPNotFoundError(ttpID string) *NotFoundError {
	return &NotFoundError{
		ResourceType: "TTP",
		ResourceID:   ttpID,
	}
}

// NewTargetNotFoundError creates a NotFoundError for a target entity
func NewTargetNotFoundError(targetID string) *NotFoundError {
	return &NotFoundError{
		ResourceType: "Target",
		ResourceID:   targetID,
	}
}

// NewProcedureNotFoundError creates a NotFoundError for a procedure
func NewProcedureNotFoundError(procedureID string) *NotFoundError {
	return &NotFoundError{
		ResourceType: "Procedure",
		ResourceID:   procedureID,
	}
}

// IsNotFoundError checks if an error is a NotFoundError
func IsNotFoundError(err error) bool {
	_, ok := err.(*NotFoundError)
	return ok
}
