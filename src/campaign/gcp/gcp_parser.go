package gcp

import (
	"encoding/json"
	"fmt"

	"github.com/Magier/Ran/domain"
)

func ParseServiceAccount(data string) (domain.GCPServiceAccount, error) {
	var gcpSA domain.GCPServiceAccount
	err := json.Unmarshal([]byte(data), &gcpSA)
	if err != nil {
		return domain.GCPServiceAccount{}, fmt.Errorf("Failed to unmarshal GCP Service Account JSON: %w", err)
	}
	gcpSA.Kind = gcpSA.GetKind() // API does not return kind, set it here (use GetKind method to have 1 source of truth)
	return gcpSA, nil
}

func ParseBuckets(data string) ([]domain.GCPBucket, error) {
	var buckets domain.GCPBucketList
	err := json.Unmarshal([]byte(data), &buckets)
	if err != nil {
		return nil, fmt.Errorf("Failed to unmarshal GCP Buckets JSON: %w", err)
	}

	return buckets.Items, nil
}
