package api

import (
	"encoding/json"
	"testing"
)

func TestSnapshotValue_ConcurrentMapAccess(t *testing.T) {
	// Create a struct with a map that we'll modify concurrently
	type Event struct {
		Name string
		Args map[string]string
	}

	original := Event{
		Name: "test",
		Args: map[string]string{
			"key1": "value1",
			"key2": "value2",
		},
	}

	// Create a snapshot
	snapshot := snapshotValue(original)

	// Modify the original map
	original.Args["key3"] = "value3"
	original.Args["key1"] = "modified"

	// Marshal the snapshot - should not panic or see the modifications
	data, err := json.Marshal(snapshot)
	if err != nil {
		t.Fatalf("Failed to marshal snapshot: %v", err)
	}

	// Unmarshal and verify the snapshot is unchanged
	var result Event
	if err := json.Unmarshal(data, &result); err != nil {
		t.Fatalf("Failed to unmarshal: %v", err)
	}

	if result.Args["key1"] != "value1" {
		t.Errorf("Expected key1=value1, got %s", result.Args["key1"])
	}
	if result.Args["key3"] != "" {
		t.Errorf("Expected key3 to be empty, got %s", result.Args["key3"])
	}
	if len(result.Args) != 2 {
		t.Errorf("Expected 2 keys in snapshot, got %d", len(result.Args))
	}
}

func TestSnapshotValue_NestedStructs(t *testing.T) {
	type Inner struct {
		Data map[string]int
	}
	type Outer struct {
		Name  string
		Inner *Inner
		List  []string
	}

	original := Outer{
		Name: "test",
		Inner: &Inner{
			Data: map[string]int{"a": 1, "b": 2},
		},
		List: []string{"x", "y"},
	}

	snapshot := snapshotValue(original).(Outer)

	// Modify original
	original.Inner.Data["c"] = 3
	original.List = append(original.List, "z")

	// Verify snapshot is independent
	if len(snapshot.Inner.Data) != 2 {
		t.Errorf("Expected 2 items in snapshot map, got %d", len(snapshot.Inner.Data))
	}
	if len(snapshot.List) != 2 {
		t.Errorf("Expected 2 items in snapshot list, got %d", len(snapshot.List))
	}
}

func TestSafeJSONMarshal_WithSnapshot(t *testing.T) {
	type TestEvent struct {
		ID   string
		Args map[string]string
	}

	event := TestEvent{
		ID: "test-123",
		Args: map[string]string{
			"param1": "value1",
			"param2": "value2",
		},
	}

	// This should not panic even if we modify the map during marshaling
	data, err := safeJSONMarshal(event)
	if err != nil {
		t.Fatalf("safeJSONMarshal failed: %v", err)
	}

	if len(data) == 0 {
		t.Error("Expected non-empty marshaled data")
	}

	// Verify it's valid JSON
	var result TestEvent
	if err := json.Unmarshal(data, &result); err != nil {
		t.Fatalf("Failed to unmarshal result: %v", err)
	}

	if result.ID != event.ID {
		t.Errorf("Expected ID %s, got %s", event.ID, result.ID)
	}
}

func TestSnapshotValue_InterfaceField(t *testing.T) {
	// Simulate the real crash: a struct with an interface field whose concrete
	// value contains a map. Without the reflect.Interface case in
	// snapshotReflectValue the map is NOT deep-copied and a concurrent write
	// during marshal causes a fatal error.
	type Inner struct {
		Labels map[string]string
	}
	type Wrapper struct {
		Name   string
		Target interface{} // mirrors domain.Entity being an interface
	}

	original := Wrapper{
		Name: "test",
		Target: Inner{
			Labels: map[string]string{"env": "prod"},
		},
	}

	snapshot := snapshotValue(original).(Wrapper)

	// Mutate the original map — snapshot must be isolated
	original.Target.(Inner).Labels["env"] = "MUTATED"
	original.Target.(Inner).Labels["new"] = "key"

	data, err := json.Marshal(snapshot)
	if err != nil {
		t.Fatalf("Marshal failed: %v", err)
	}

	var result Wrapper
	if err := json.Unmarshal(data, &result); err != nil {
		t.Fatalf("Unmarshal failed: %v", err)
	}

	inner := result.Target.(map[string]interface{})
	labels := inner["Labels"].(map[string]interface{})
	if labels["env"] != "prod" {
		t.Errorf("Expected env=prod, got %v", labels["env"])
	}
	if _, exists := labels["new"]; exists {
		t.Error("Snapshot should not contain key 'new' added after snapshot")
	}
}
