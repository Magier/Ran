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
	Args        map[string]string
	Success     bool
	Command     string
	Results     []string
	StartAt     time.Time
	Target      domain.Entity
	ExecutedOn  domain.System
	CompletedAt time.Time
	Observables []any
	ExecCommand domain.ExecTTP
}

type AuditTrail struct {
	steps     []AttackStep
	openSteps []AttackStep
}

func NewAuditTrail() AuditTrail {
	return AuditTrail{}
}

func (a *AuditTrail) Reset() {
	a.steps = make([]AttackStep, 0)
	a.openSteps = make([]AttackStep, 0)
}

func (a *AuditTrail) AddNewStep(action domain.ExecTTP) error {
	var execOn domain.System
	if action.C2Channel != nil {
		// TODO: check if there is a difference between the ExecutedOn set by C2 and the terminal point of the C2 Channel set in campaign
		execOn = action.C2Channel.GetFinalTarget().(domain.System)
	}
	a.openSteps = append(a.openSteps, AttackStep{
		ExecCommand: action,
		ID:          action.ID,
		TTP:         action.TTP,
		Args:        action.Args,
		Target:      action.Target,
		ExecutedOn:  execOn,
		Command:     action.Procedure.Command,
		StartAt:     time.Now(),
	})
	return nil
}

func (a *AuditTrail) GetOpenStep(id string) (AttackStep, bool) {
	for _, step := range a.openSteps {
		if step.ID == id {
			return step, true
		}
	}
	return AttackStep{}, false
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

func (a *AuditTrail) CompleteStep(id string, ttp domain.TTP, success bool, results []string) bool {
	step, ok := a.popOpenStep(id)

	if ok {
		step.CompletedAt = time.Now()
		step.Success = success
		step.Results = results

		if ttp.Defense.ID != "" {
			// TODO: see how actual IoCs, etc. can be added to the detection
			if ttp.Defense.Sigma != nil {
				step.Observables = append(step.Observables, *ttp.Defense.Sigma)
			}
		}
		a.steps = append(a.steps, step)
	} else {
		slog.Warn(fmt.Sprintf("Could not pop open attack step %s", id))
	}
	return ok
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
		action, observables = attackflow.NewAttackAction(s.TTP.Name, s.TTP.Description, string(s.TTP.Tactic), technique, s.Command, s.StartAt, s.CompletedAt)
		// TODO link the observable
		obj = obj.Append(action)
		bundle.Objects = bundle.Objects.Append(observables...)

		if s.TTP.Defense.ID != "" && s.TTP.Defense.Sigma != nil {
			rule := *s.TTP.Defense.Sigma
			yamlStr, err := rule.ToYAMLString()

			if err != nil {
				slog.Error("Could not convert sigma rule to YAML", "error", err)
			} else {
				observables = append(observables, attackflow.NewIndicator(
					rule.Title,
					rule.Description,
					yamlStr,
					"sigma",
					s.StartAt,
					s.CompletedAt,
				))
			}
		}

		for _, obs := range observables {
			// TODO: if the action produced another observable than an indicator (e.g. a process), then link to that one instead
			bundle.Objects = bundle.Objects.Append(obs, attackflow.NewRelationship(action.ID, obs.GetID(), "indicates", action.Created))
		}

		if s.Target != nil {
			targetID := s.Target.GetId()
			if assetID, ok := knownAssets[s.Target.GetId()]; ok {
				if _, ok := s.Target.(domain.C2System); ok {
					infraRel := attackflow.NewRelationship(action.ID, assetID, attackflow.RelatedTo, action.Created)
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
					infraRel := attackflow.NewRelationship(action.ID, asset.ID, attackflow.RelatedTo, action.Created)
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
