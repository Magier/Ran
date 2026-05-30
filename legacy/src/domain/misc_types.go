package domain

import (
	"fmt"

	k8s_types "github.com/Magier/Ran/k8sclient/types"
)

type SelfSubjectRulesReview struct {
	TokenName        string
	ServiceAccount   ServiceAccount
	Result           string
	ResourceRules    []k8s_types.ResourceRule
	NonResourceRules []k8s_types.NonResourceRule
}

// GetId implements Entity.
func (s SelfSubjectRulesReview) GetId() string {
	return fmt.Sprintf("SSRR/%s", s.TokenName)
}

// GetName implements Entity.
func (s SelfSubjectRulesReview) GetName() string {
	return fmt.Sprintf("SelfSubjectRulesReview for %s", s.TokenName)
}

func (s SelfSubjectRulesReview) GetKind() string {
	return "SelfSubjectRulesReview"
}

// TODO: temporary workaround to streamline processing of information
var _ Entity = (*SelfSubjectRulesReview)(nil)
