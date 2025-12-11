package domain

import "fmt"

type CloudEnvironment struct {
	Provider string `json:"provider"`
	Name     string `json:"name"`
	// Region   string `json:"region"`
	// Account  string `json:"account"`
}

var _ Entity = (*CloudEnvironment)(nil)

// GetId implements Entity.
func (c CloudEnvironment) GetId() string {
	return fmt.Sprintf("%s/%s", c.Provider, c.Name)
}

// GetKind implements Entity.
func (c CloudEnvironment) GetKind() string {
	return c.Provider
}

// GetName implements Entity.
func (c CloudEnvironment) GetName() string {
	return c.Name
}

type GCPServiceAccountToken struct {
	Name      *string `json:"name"`
	Token     string  `json:"access_token"`
	ExpiresIn int64   `json:"expires_in"`
	Type      string  `json:"token_type"`
}

var _ Entity = (*GCPServiceAccountToken)(nil)

// GetId implements Entity.
func (g GCPServiceAccountToken) GetId() string {
	name := "default"
	if g.Name != nil {
		name = *g.Name
	}
	return fmt.Sprintf("gcp-sa/%s", name)
}

// GetKind implements Entity.
func (g GCPServiceAccountToken) GetKind() string {
	return "GCPServiceAccountToken"
}

// GetName implements Entity.
func (g GCPServiceAccountToken) GetName() string {
	name := "default"
	if g.Name != nil {
		name = *g.Name
	}
	return "gcp-sa-token-" + name
}

// GetName implements Entity.
func (g GCPServiceAccountToken) String() string {
	return "GCP Service Account: " + g.Token
}
