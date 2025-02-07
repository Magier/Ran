package campaign

import (
	"fmt"
	"log/slog"
	"time"

	"github.com/Magier/Ran/domain"
)

type AttackStep struct {
	ID          string
	TTP         domain.TTP
	Success     bool
	Description string
	StartAt     time.Time
	CompletedAt time.Time
	Observables []any
}

type AuditTrail struct {
	steps     []AttackStep
	openSteps []AttackStep
}

func NewAuditTrail() AuditTrail {
	return AuditTrail{}
}

func (a *AuditTrail) AddNewStep(id string, ttp domain.TTP) error {
	a.openSteps = append(a.openSteps, AttackStep{
		ID:      id,
		TTP:     ttp,
		StartAt: time.Now(),
	})
	return nil
}

func (a *AuditTrail) popOpenStep(id string) (AttackStep, bool) {
	for i, step := range a.openSteps {
		if step.ID == id {
			a.openSteps = append(a.openSteps[:i], a.openSteps[i+1:]...)

			return step, true
		}
	}
	return AttackStep{}, false
}

func (a *AuditTrail) CompleteStep(id string, ttp domain.TTP, success bool) {
	step, ok := a.popOpenStep(id)

	if ok {
		step.CompletedAt = time.Now()
		step.Success = success
		step.Description = "temp description"
		// # TODO: enrich observables depending on the TTP
		// step.Observables = append(step.Observables, )
		a.steps = append(a.steps, step)
	} else {
		slog.Warn(fmt.Sprintf("Could not pop open attack step %s", id))
	}
}

func (a AuditTrail) Export(ttp domain.TTP, success bool) error {
	return nil
}
