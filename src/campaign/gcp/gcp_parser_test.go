package gcp

import (
	"fmt"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestParseServiceAccount(t *testing.T) {
	email := "10123-compute@developer.gserviceaccount.com"
	token := "ya29.c.accesstoken"
	expiresIn := 1337

	tests := []struct {
		name          string
		data          string
		wantErr       bool
		expectToken   string
		expectEmail   string
		expectExpires int64
	}{
		{
			name:          "valid service account JSON",
			data:          fmt.Sprintf(`{"email":"%s","token": {"access_token":"%s", "expires_in": %d, "token_type":"Bearer"}}`, email, token, expiresIn),
			wantErr:       false,
			expectEmail:   email,
			expectToken:   token,
			expectExpires: int64(expiresIn),
		},
		{
			name:    "invalid JSON",
			data:    `{invalid json}`,
			wantErr: true,
		},
		{
			name:    "empty JSON",
			data:    `{}`,
			wantErr: false,
		},
		{
			name:    "empty string",
			data:    ``,
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := ParseServiceAccount(tt.data)
			if tt.wantErr {
				assert.Error(t, err)
			} else {
				assert.NoError(t, err)
				assert.NotNil(t, result)
				assert.Equal(t, tt.expectEmail, result.EMail)
				assert.Equal(t, tt.expectToken, result.Token.Token)
				assert.Equal(t, tt.expectExpires, result.Token.ExpiresIn)
			}
		})
	}
}

func TestParseBuckets(t *testing.T) {
	tests := []struct {
		name       string
		data       string
		wantErr    bool
		wantLength int
	}{
		{
			name:       "valid buckets JSON with items",
			data:       `{"kind":"storage#buckets","items":[{"kind":"storage#bucket","id":"bucket1","name":"test-bucket-1"},{"kind":"storage#bucket","id":"bucket2","name":"test-bucket-2"}]}`,
			wantErr:    false,
			wantLength: 2,
		},
		{
			name:       "valid buckets JSON with no items",
			data:       `{"kind":"storage#buckets","items":[]}`,
			wantErr:    false,
			wantLength: 0,
		},
		{
			name:       "valid buckets JSON with null items",
			data:       `{"kind":"storage#buckets"}`,
			wantErr:    false,
			wantLength: 0,
		},
		{
			name:       "invalid JSON",
			data:       `{invalid json}`,
			wantErr:    true,
			wantLength: 0,
		},
		{
			name:       "empty string",
			data:       ``,
			wantErr:    true,
			wantLength: 0,
		},
		{
			name:       "empty JSON object",
			data:       `{}`,
			wantErr:    false,
			wantLength: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := ParseBuckets(tt.data)
			if tt.wantErr {
				assert.Error(t, err)
				assert.Nil(t, result)
			} else {
				assert.NoError(t, err)
				assert.Len(t, result, tt.wantLength)
			}
		})
	}
}
