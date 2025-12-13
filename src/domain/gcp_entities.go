package domain

import (
	"fmt"
)

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
	Kind      string  `json:"kind"`
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

// {
//       "kind": "storage#bucket",
//       "selfLink": "https://www.googleapis.com/storage/v1/b/1092480157775-europe-central2-blueprint-config",
//       "id": "1092480157775-europe-central2-blueprint-config",
//       "name": "1092480157775-europe-central2-blueprint-config",
//       "projectNumber": "1092480157775",
//       "generation": "1761564485727658362",
//       "metageneration": "1",
//       "location": "EUROPE-CENTRAL2",
//       "storageClass": "STANDARD",
//       "etag": "CAE=",
//       "timeCreated": "2025-10-27T11:28:06.128Z",
//       "updated": "2025-10-27T11:28:06.128Z",
//       "softDeletePolicy": {
//         "retentionDurationSeconds": "604800",
//         "effectiveTime": "2025-10-27T11:28:06.128Z"
//       },
//       "iamConfiguration": {
//         "bucketPolicyOnly": {
//           "enabled": false
//         },
//         "uniformBucketLevelAccess": {
//           "enabled": false
//         },
//         "publicAccessPrevention": "inherited"
//       },
//       "locationType": "region",
//       "satisfiesPZS": true,
//       "satisfiesPZI": true
//     }

type GCPBucket struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	Kind     string `json:"kind"`
	SelfLink string `json:"selfLink"`
	Location string `json:"location"`
	// ProjectNumber string `json:"projectNumber"`
	TimeCreated string `json:"timeCreated"`
	Updated     string `json:"updated"`
}

var _ Entity = (*GCPBucket)(nil)

// GetId implements Entity.
func (g GCPBucket) GetId() string {
	return fmt.Sprintf("gcp/bucket/%s", g.ID)
}

// GetKind implements Entity.
func (g GCPBucket) GetKind() string {
	return "GCPBucket"
	// return g.Kind
}

// GetName implements Entity.
func (g GCPBucket) GetName() string {
	return g.Name
}

type GCPBucketList struct {
	Kind  string      `json:"kind"`
	Items []GCPBucket `json:"items"`
}
