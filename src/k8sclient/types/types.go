package k8s_types

import (
	"fmt"
	"time"
)

// Source: https://github.com/kubernetes/kubernetes/blob/2d0a4f75560154454682b193b42813159b20f284/pkg/apis/authorization/types.go#L277

// NonResourceRule holds information that describes a rule for the non-resource
type NonResourceRule struct {
	// Verb is a list of kubernetes non-resource API verbs, like: get, post, put, delete, patch, head, options.  "*" means all.
	Verbs []string `json:"verbs"`

	// NonResourceURLs is a set of partial urls that a user should have access to.  *s are allowed, but only as the full,
	// final step in the path.  "*" means all.
	NonResourceURLs []string `json:"nonResourceURLs"`
}

// ResourceRule is the list of actions the subject is allowed to perform on resources. The list ordering isn't significant,
// may contain duplicates, and possibly be incomplete.
type ResourceRule struct {
	// Verb is a list of kubernetes resource API verbs, like: get, list, watch, create, update, delete, proxy.  "*" means all.
	Verbs []string `json:"verbs"`
	// APIGroups is the name of the APIGroup that contains the resources.  If multiple API groups are specified, any action requested against one of
	// the enumerated resources in any API group will be allowed.  "*" means all.
	APIGroups []string `json:"apiGroups"`
	// Resources is a list of resources this rule applies to.  "*" means all in the specified apiGroups.
	//  "*/foo" represents the subresource 'foo' for all resources in the specified apiGroups.
	Resources []string `json:"resources"`
	// ResourceNames is an optional white list of names that the rule applies to.  An empty set means that everything is allowed.  "*" means all.
	ResourceNames []string `json:"resourceNames"`
}

// SubjectRulesReviewStatus contains the result of a rules check. This check can be incomplete depending on
// the set of authorizers the server is configured with and any errors experienced during evaluation.
// Because authorization rules are additive, if a rule appears in a list it's safe to assume the subject has that permission,
// even if that list is incomplete.
type SubjectRulesReviewStatus struct {
	// ResourceRules is the list of actions the subject is allowed to perform on resources.
	// The list ordering isn't significant, may contain duplicates, and possibly be incomplete.
	ResourceRules []ResourceRule `json:"resourceRules,omitempty"`
	// NonResourceRules is the list of actions the subject is allowed to perform on non-resources.
	// The list ordering isn't significant, may contain duplicates, and possibly be incomplete.
	NonResourceRules []NonResourceRule `json:"nonResourceRules,omitempty"`
	// Incomplete is true when the rules returned by this call are incomplete. This is most commonly
	// encountered when an authorizer, such as an external authorizer, doesn't support rules evaluation.
	Incomplete bool `json:"incomplete,omitempty"`
	// EvaluationError can appear in combination with Rules. It indicates an error occurred during
	// rule evaluation, such as an authorizer that doesn't support rule evaluation, and that
	// ResourceRules and/or NonResourceRules may be incomplete.
	EvaluationError string `json:"evaluationError,omitempty"`
}

// SelfSubjectRulesReviewSpec defines the specification for SelfSubjectRulesReview.
type SelfSubjectRulesReviewSpec struct {
	// Namespace to evaluate rules for. Required.
	Namespace string `json:"namespace,omitempty"`
}

// "{\n  \"kind\": \"SelfSubjectRulesReview\",\n  \"apiVersion\": \"authorization.k8s.io/v1\",\n  \"metadata\": {\n    \"creationTimestamp\": null\n  },\n  \"spec\": {},\n  \"status\": {\n    \"resourceRules\": [\n      {\n        \"verbs\": [\n          \"create\"\n        ],\n        \"apiGroups\": [\n          \"authorization.k8s.io\"\n        ],\n        \"resources\": [\n          \"selfsubjectaccessreviews\",\n          \"selfsubjectrulesreviews\"\n        ]\n      },\n      {\n        \"verbs\": [\n          \"create\"\n        ],\n        \"apiGroups\": [\n          \"authentication.k8s.io\"\n        ],\n        \"resources\": [\n          \"selfsubjectreviews\"\n        ]\n      },\n      {\n        \"verbs\": [\n          \"get\",\n          \"list\",\n          \"create\"\n        ],\n        \"apiGroups\": [\n          \"\"\n        ],\n        \"resources\": [\n          \"pods\",\n          \"deployment\"\n        ]\n      },\n      {\n        \"verbs\": [\n          \"create\"\n        ],\n        \"apiGroups\": [\n          \"\"\n        ],\n        \"resources\": [\n          \"pods/exec\"\n        ]\n      }\n    ],\n    \"nonResourceRules\": [\n      {\n        \"verbs\": [\n          \"get\"\n        ],\n        \"nonResourceURLs\": [\n          \"/api\",\n          \"/api/*\",\n          \"/apis\",\n          \"/apis/*\",\n          \"/healthz\",\n          \"/livez\",\n          \"/openapi\",\n          \"/openapi/*\",\n          \"/readyz\",\n          \"/version\",\n          \"/version/\"\n        ]\n      },\n      {\n        \"verbs\": [\n          \"get\"\n        ],\n        \"nonResourceURLs\": [\n          \"/healthz\",\n          \"/livez\",\n          \"/readyz\",\n          \"/version\",\n          \"/version/\"\n        ]\n      },\n      {\n        \"verbs\": [\n          \"get\"\n        ],\n        \"nonResourceURLs\": [\n          \"/.well-known/openid-configuration\",\n          \"/.well-known/openid-configuration/\",\n          \"/openid/v1/jwks\",\n          \"/openid/v1/jwks/\"\n        ]\n      }\n    ],\n    \"incomplete\": false\n  }\n}"
type SelfSubjectRulesReview struct {
	Kind       string `json:"kind"`
	APIVersion string `json:"apiVersion"`
	Metadata   struct {
		CreationTimestamp *time.Time `json:"creationTimestamp,omitempty"`
	} `json:"metadata"`
	Spec    SelfSubjectRulesReviewSpec `json:"spec,omitempty"`
	Status  SubjectRulesReviewStatus   `json:"status,omitempty"`
	Message string                     `json:"message,omitempty"`
	Reason  string                     `json:"reason,omitempty"`
	Details struct {
		Group string `json:"group,omitempty"`
		Kind  string `json:"kind,omitempty"`
	} `json:"details,omitempty"`
	Code int `json:"code,omitempty"`
}

type K8sApiResponseStatus struct {
	Code    int    `json:"code,omitempty"`
	Message string `json:"message,omitempty"`
	Reason  string `json:"reason,omitempty"`
	Status  string `json:"status,omitempty"`
}

type K8sAPIResponseError struct {
	Status K8sApiResponseStatus `json:"status,omitempty"`
}

func (s K8sAPIResponseError) Error() string {
	return fmt.Sprintf("K8s API error %d: %s", s.Status.Code, s.Status.Message)
}
