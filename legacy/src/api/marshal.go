package api

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"reflect"
)

// snapshotValue creates a deep copy of maps and slices to avoid concurrent access issues
// during JSON marshaling. This prevents the "concurrent map iteration and map write" panic.
func snapshotValue(v interface{}) interface{} {
	if v == nil {
		return nil
	}

	val := reflect.ValueOf(v)
	return snapshotReflectValue(val).Interface()
}

func snapshotReflectValue(val reflect.Value) reflect.Value {
	// Handle invalid values
	if !val.IsValid() {
		return val
	}

	switch val.Kind() {
	case reflect.Ptr:
		if val.IsNil() {
			return val
		}
		// Create new pointer and copy the underlying value
		elem := val.Elem()
		newPtr := reflect.New(elem.Type())
		newPtr.Elem().Set(snapshotReflectValue(elem))
		return newPtr

	case reflect.Interface:
		if val.IsNil() {
			return val
		}
		// Unwrap the interface, snapshot the underlying concrete value, and re-wrap
		elem := val.Elem()
		snapped := snapshotReflectValue(elem)
		// Re-wrap into an interface value of the original interface type
		wrapper := reflect.New(val.Type()).Elem()
		wrapper.Set(snapped)
		return wrapper

	case reflect.Map:
		if val.IsNil() {
			return val
		}
		// Create a new map and copy all key-value pairs
		newMap := reflect.MakeMap(val.Type())
		iter := val.MapRange()
		for iter.Next() {
			k := iter.Key()
			v := iter.Value()
			newMap.SetMapIndex(k, snapshotReflectValue(v))
		}
		return newMap

	case reflect.Slice:
		if val.IsNil() {
			return val
		}
		// Create a new slice and copy all elements
		newSlice := reflect.MakeSlice(val.Type(), val.Len(), val.Cap())
		for i := 0; i < val.Len(); i++ {
			newSlice.Index(i).Set(snapshotReflectValue(val.Index(i)))
		}
		return newSlice

	case reflect.Struct:
		// Create a new struct and copy all fields
		newStruct := reflect.New(val.Type()).Elem()
		for i := 0; i < val.NumField(); i++ {
			field := val.Field(i)
			if field.CanInterface() && newStruct.Field(i).CanSet() {
				newStruct.Field(i).Set(snapshotReflectValue(field))
			}
		}
		return newStruct

	default:
		// For basic types (int, string, bool, etc.), just return as-is
		return val
	}
}

// safeJSONMarshal wraps json.Marshal with recovery from panics caused by
// concurrent map read/write during marshalling. Event structs may contain maps
// that are concurrently modified by other goroutines since the message bus
// dispatches handlers in parallel.
func safeJSONMarshal(v interface{}) (data []byte, err error) {
	defer func() {
		if r := recover(); r != nil {
			slog.Warn("Recovered from panic during JSON marshal", "panic", r)
			err = fmt.Errorf("concurrent map access during marshal: %v", r)
			data = nil
		}
	}()

	// Create a snapshot to avoid concurrent access issues
	snapshot := snapshotValue(v)
	return json.Marshal(snapshot)
}
