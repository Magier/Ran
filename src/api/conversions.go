package api

import (
	"fmt"
	"log/slog"

	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
)

func ConvertAttackStep(step campaign.AttackStep) AttackStep {
	procID := step.ExecCommand.Procedure.Key

	executedOn := ""
	if step.ExecutedOn != nil {
		executedOn = step.ExecutedOn.GetId()
	}

	var targetId string
	if step.Target != nil {
		targetId = step.Target.GetId()
	} else {
		slog.Error(fmt.Sprintf("The target of attack step %s was nil :O", step.TTP.Name))
	}

	var defense *TTPDefense
	if step.TTP.Defense.ID != "" {
		d := ConvertTTPDefense(step.TTP.Defense)
		defense = &d
	}

	s := AttackStep{
		Id:          step.ID,
		TTP:         ConvertTTP(step.TTP),
		Args:        step.Args,
		TargetId:    targetId,
		Success:     step.Success,
		Command:     step.Command,
		ProcedureId: procID,
		Results:     step.Results,
		ExecutedOn:  executedOn,
		StartedAt:   step.StartAt,
		CompletedAt: step.CompletedAt,
		Defense:     defense,
		// Observables: ConvertObservables(step.Observables),
		// ExecCommand: ConvertExecTTP(step.ExecCommand),
	}
	return s
}

func ConvertTTP(ttp domain.TTP) TTP {
	var effects *[]string
	if len(ttp.Effects) > 0 {
		effects = &ttp.Effects
	}

	return TTP{
		Id:          ttp.ID,
		Name:        ttp.Name,
		Description: ttp.Description,
		Tactic:      string(ttp.Tactic),
		Techniques:  ttp.Techniques,
		Requires:    ConvertRequirements(ttp.Requires),
		Effects:     effects,
		Procedures:  ConvertProcedures(ttp.Procedures),
		Params:      ConvertTTPParams(ttp.Params),
		Status:      TTPStatus(ttp.Status),
	}
}

func ConvertTTPDefense(defense domain.Defense) TTPDefense {
	return TTPDefense{
		Id:          defense.ID,
		Name:        defense.Name,
		Url:         &defense.URL,
		Description: &defense.Description,
		D3efend:     &defense.D3fend,
		Sigma:       nil,
		// Description: defense.Description,
		// Procedures:  ConvertProcedures(defense.Procedures),
	}
}

func ConvertTTPParams(parameter []domain.Parameter) []TTPParam {
	params := make([]TTPParam, 0)
	for _, p := range parameter {
		param := TTPParam{
			Name:        p.Name,
			Type:        p.Type,
			Description: p.Description,
			Required:    p.Required,
			Default:     p.Default,
		}
		params = append(params, param)
	}
	return params
}

func ConvertProcedures(procedure []domain.Procedure) []Procedure {
	procedures := make([]Procedure, 0)
	for _, p := range procedure {
		proc := Procedure{
			Id:      p.Key,
			Command: p.Command,
			Tool:    &p.Tool,
		}
		procedures = append(procedures, proc)
	}
	return procedures
}

func ConvertRequirements(r domain.Requirements) Requirements {
	kind := string(r.Kind)
	accessLevel := r.AccessLevel.String()
	rbacPermissions := []RBACPermission{}

	if len(r.RBACPermissions) > 0 {
		for _, perm := range r.RBACPermissions {
			rbacPermissions = append(rbacPermissions, ConvertRBACPermission(perm))
		}
	}

	var exists *[]string
	if len(r.Exists) > 0 {
		exists = (*[]string)(&r.Exists)
	}

	return Requirements{
		Kind:            &kind,
		AccessLevel:     &accessLevel,
		Exists:          exists,
		RbacPermissions: &rbacPermissions,
	}
}

func ConvertRBACPermission(r domain.RBACPermission) RBACPermission {
	return RBACPermission{
		Verb:         r.Verb,
		ResourceName: r.ResourceName,
		ResourceType: r.ResourceType,
		ApiGroup:     r.APIGroup,
		Scope:        r.Scope,
		SourceRole:   r.SourceRole,
	}
}

// var _ RBACPermission = (*domain.RBACPermission)(domain.RBACPermission{})
