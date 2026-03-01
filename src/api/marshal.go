package api

import (
	"encoding/json"
	"fmt"
	"log/slog"
)

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
	return json.Marshal(v)
}
