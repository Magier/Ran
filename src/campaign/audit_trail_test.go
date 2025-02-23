package campaign

import (
	"testing"

	"github.com/Magier/Ran/domain"
	"github.com/stretchr/testify/assert"
)

func TestNewAuditTrail(t *testing.T) {
	auditTrail := NewAuditTrail()
	assert.NotNil(t, auditTrail)
	assert.Empty(t, auditTrail.steps)
	assert.Empty(t, auditTrail.openSteps)
}

func TestAddNewStep(t *testing.T) {
	auditTrail := NewAuditTrail()
	ttp := domain.TTP{ID: "test-ttp"}

	err := auditTrail.AddNewStep("step1", ttp)
	assert.NoError(t, err)
	assert.Len(t, auditTrail.openSteps, 1)
	assert.Equal(t, "step1", auditTrail.openSteps[0].ID)
	assert.Equal(t, ttp, auditTrail.openSteps[0].TTP)
}

func TestCompleteStep(t *testing.T) {
	auditTrail := NewAuditTrail()
	ttp := domain.TTP{ID: "test-ttp"}

	err := auditTrail.AddNewStep("step1", ttp)
	assert.NoError(t, err)

	auditTrail.CompleteStep("step1", ttp, true)
	assert.Len(t, auditTrail.steps, 1)
	assert.Empty(t, auditTrail.openSteps)
	assert.Equal(t, "step1", auditTrail.steps[0].ID)
	assert.Equal(t, ttp, auditTrail.steps[0].TTP)
	assert.True(t, auditTrail.steps[0].Success)
	assert.NotZero(t, auditTrail.steps[0].CompletedAt)
}

func TestConvertToAttackFlow(t *testing.T) {
	auditTrail := NewAuditTrail()
	ttp := domain.TTP{ID: "test-ttp"}

	err := auditTrail.AddNewStep("initial access", ttp)
	assert.NoError(t, err)
	auditTrail.CompleteStep("initial access", ttp, true)

	err = auditTrail.AddNewStep("lateral movement", ttp)
	assert.NoError(t, err)
	auditTrail.CompleteStep("lateral movement", ttp, true)

	af, err := auditTrail.ConvertToAttackFlow()
	assert.NoError(t, err)
	assert.NotNil(t, af)
	// Add more assertions based on the expected structure of attackflow.StixBundle
}
