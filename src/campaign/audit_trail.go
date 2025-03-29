package campaign

import (
	"errors"
	"fmt"
	"log/slog"
	"time"

	"github.com/Magier/Ran/domain"
	attackflow "github.com/Magier/Ran/mitre/attack_flow"
)

type AttackStep struct {
	ID          string
	TTP         domain.TTP
	Success     bool
	Command     string
	Description string
	StartAt     time.Time
	Target      domain.Entity
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

func (a *AuditTrail) AddNewStep(action domain.ExecTTP) error {
	a.openSteps = append(a.openSteps, AttackStep{
		ID:      action.ID,
		TTP:     action.TTP,
		Target:  action.Target,
		Command: action.Variant.Command,
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
	bundle.Objects = bundle.Objects.Append(creator)

	var obj attackflow.AttackFlowObject
	obj = af
	var action attackflow.AttackAction
	var observables []attackflow.StixObject

	knownAssets := map[string]string{}

	for _, s := range a.steps {
		var technique string
		if len(technique) > 0 {
			technique = s.TTP.Techniques[0]
		}
		action, observables = attackflow.NewAttackAction(s.TTP.Name, s.Description, string(s.TTP.Tactic), technique, s.Command, s.StartAt, s.CompletedAt)
		// TODO link the observable
		obj = obj.Append(action)
		bundle.Objects = bundle.Objects.Append(observables...)

		if s.Target != nil {
			targetID := s.Target.GetId()
			if assetID, ok := knownAssets[s.Target.GetId()]; ok {
				if _, ok := s.Target.(domain.C2System); ok {
					infraRel := attackflow.Newrelationship(action.ID, assetID, attackflow.RelatedTo)
					bundle.Objects = bundle.Objects.Append(infraRel)
				} else {
					action.AssetRefs = append(action.AssetRefs, assetID)
				}
			} else { // create a new asset/infrastructure
				if c2, ok := s.Target.(domain.C2System); ok {
					asset := attackflow.Infrastructure{
						SDO:   attackflow.NewSDO("infrastructure", c2.Name, "", false),
						Types: []string{attackflow.InfraTypeC2},
					}
					infraRel := attackflow.Newrelationship(action.ID, asset.ID, attackflow.RelatedTo)
					bundle.Objects = bundle.Objects.Append(asset, infraRel)
					knownAssets[targetID] = asset.ID
				} else {
					asset := attackflow.NewAttackAsset(s.Target.GetId(), s.Target.GetName())
					action.AssetRefs = append(action.AssetRefs, asset.ID)
					bundle.Objects = bundle.Objects.Append(asset)
					knownAssets[targetID] = asset.ID
				}
			}
		}

		if s.Success {
			// no more descendants expected, add it to the final list of objects
			bundle.Objects = bundle.Objects.Append(obj)
			obj = action
		} else {
			// if it failed, then it's a dead "branch", just add the action directly
			bundle.Objects = bundle.Objects.Append(action)
		}
	}

	// ensure last action is also added
	bundle.Objects = bundle.Objects.Append(action)
	return bundle, nil
}

func (a AuditTrail) GetSteps() []AttackStep {
	return a.steps
}
