package campaign

import (
	"errors"
	"fmt"
	"log/slog"
	"time"

	attackflow "github.com/Magier/Ran/attack_flow"
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

func (a *AuditTrail) CompleteStep(id string, ttp domain.TTP, success bool, descr string) {
	step, ok := a.popOpenStep(id)

	if ok {
		step.CompletedAt = time.Now()
		step.Success = success
		step.Description = descr
		// # TODO: enrich observables depending on the TTP
		// step.Observables = append(step.Observables, )
		a.steps = append(a.steps, step)
	} else {
		slog.Warn(fmt.Sprintf("Could not pop open attack step %s", id))
	}
}

func (a AuditTrail) ConvertToAttackFlow() (attackflow.StixBundle, error) {
	if len(a.steps) == 0 {
		return attackflow.StixBundle{}, errors.New("A valid AttackFlow must have at list one attack-action or -condition")
	}

	bundle := attackflow.NewStixBundle()

	// TODO: properly specify name and description of the flow
	af, creator := attackflow.NewAttackFlow("Ran Campaign", "A description")
	bundle.Objects = append(bundle.Objects, creator)

	var obj attackflow.AttackFlowObject
	obj = af
	var action attackflow.AttackAction
	for _, s := range a.steps {
		var technique string
		if len(technique) > 0 {
			technique = s.TTP.Technique[0]
		}
		action = attackflow.NewAttackAction(s.TTP.Name, s.Description, string(s.TTP.Tactic), technique)
		action.SDO.Created = attackflow.Timestamp(s.StartAt)
		action.SDO.Modified = attackflow.Timestamp(s.CompletedAt)
		// TODO link the observable
		obj = obj.Append(action)

		if s.Success {
			// no more descendants expected, add it to the final list of objects
			bundle.Objects = append(bundle.Objects, obj)
			obj = action
		} else {
			// if it failed, then it's a dead "branch", just add the action directly
			bundle.Objects = append(bundle.Objects, action)
		}
	}

	// ensure last action is also added
	bundle.Objects = append(bundle.Objects, action)
	return bundle, nil
}
