package domain

import (
	"fmt"
	"log/slog"
	"reflect"
	"strings"
	"unicode"

	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

func GetResourceShortName(kind string) string {
	switch k := strings.ToLower(kind); k {
	case "deployment":
		return "deploy"
	case "daemonset":
		return "ds"
	case "statefulset":
		return "sts"
	case "replicaset":
		return "rs"
	case "abstractworkload", "workload":
		return "wl"
	case "service":
		return "svc"
	case "serviceaccount":
		return "sa"
	case "namespace":
		return "ns"
	case "rolebinding":
		return "rb"
	case "clusterrolebinding":
		return "crb"
	case "clusterrole":
		return "cr"
	default:
		return k
	}
}

func GetKindFromResourceShortName(short string) string {
	switch s := strings.ToLower(short); s {
	case "deploy":
		return "Deployment"
	case "depl":
		return "Deployment"
	case "ds":
		return "DaemonSet"
	case "sts":
		return "StatefulSet"
	case "rs":
		return "ReplicaSet"
	case "ns":
		return "Namespace"
	case "wl":
		return "AbstractWorkload"
	case "svc":
		return "Service"
	case "sa":
		return "ServiceAccount"
	case "rb":
		return "RoleBinding"
	case "crb":
		return "ClusterRoleBinding"
	case "cr":
		return "ClusterRole"
	default:
		return cases.Title(language.English, cases.NoLower).String(s)
	}
}

func NormalizeResourceType(kind string) string {
	if kind == "" {
		return ""
	}

	kind = strings.ToLower(kind)
	subResource := ""
	if strings.Contains(kind, "/") {
		parts := strings.Split(kind, "/")
		if len(parts) > 1 {
			subResource = parts[len(parts)-1]
			kind = strings.Join(parts[:len(parts)-1], "/")
		}
	}

	if strings.HasSuffix(kind, "y") && !strings.ContainsAny(string(kind[len(kind)-2]), "aeiou") {
		kind = kind[:len(kind)-1] + "ies"
	} else if !strings.HasSuffix(kind, "s") {
		kind = kind + "s"
	}

	// re-assembled to full resource/subresource string
	if subResource != "" {
		kind = kind + "/" + subResource
	}

	return kind
}

func CleanEventName(s string) string {
	// remove the "domain." prefix if present
	s = strings.TrimPrefix(s, "domain.")

	var result strings.Builder
	for i, r := range s {
		if unicode.IsUpper(r) {
			if i > 0 {
				prev := rune(s[i-1])
				nextLower := false
				if i+1 < len(s) {
					nextLower = unicode.IsLower(rune(s[i+1]))
				}
				if !unicode.IsUpper(prev) || nextLower {
					result.WriteRune('-')
				}
			}
			result.WriteRune(unicode.ToLower(r))
		} else if r == '.' {
			// skip - next upper case letter will add a dash
		} else {
			result.WriteRune(r)
		}
	}
	return result.String()
}

func UpdateEntity(new, old Entity) Entity {
	if reflect.DeepEqual(new, old) { // nothing to do
		return new
	}

	// Capture Pod-specific fields where false is a meaningful value (not just
	// the zero value) before merging, so they are not silently overwritten by
	// the old entity's non-zero value during the merge.
	var newPodIsRunning bool
	var newIsPod bool
	var newAccessLevel AccessLevel
	var hasIncomingAccessLevel bool
	if newPod, ok := new.(Pod); ok {
		newPodIsRunning = newPod.IsRunning
		newIsPod = true
		if newPod.SystemImpl != nil {
			newAccessLevel = newPod.SystemImpl.AccessLevel
			hasIncomingAccessLevel = true
		}
	}

	prevOwnable, oldIsOwnable := old.(Ownable)
	if ownable, ok := new.(Ownable); oldIsOwnable && ok {
		hasOwner := false
		ownerRef, _ := ownable.GetOwner()
		if ownerRef.Name == "" {
			ownerRef, hasOwner = prevOwnable.GetOwner()
		}

		if hasOwner {
			switch e := new.(type) {
			case Pod:
				e.Owner = ownerRef
				new = e
			}
		}
	}

	// 1) if old one has default value, ignore the field
	// 2) if new one has default value, use the value from the old one
	// 3) If both have a value set, use the new one (already set)
	// If both are zero, keep zero (already set)
	new = mergeEntities(new, old)

	// Restore IsRunning=false if the incoming pod explicitly signalled the pod
	// is no longer running. mergeValue treats false as a zero value and would
	// otherwise keep the old entity's true, losing the "pod stopped" signal.
	if newIsPod && !newPodIsRunning {
		if mergedPod, ok := new.(Pod); ok && mergedPod.IsRunning {
			mergedPod.IsRunning = false
			new = mergedPod
		}
	}

	// Restore AccessLevel=NoAccess if the incoming system explicitly cleared it.
	// NoAccess == {0,0} is the struct zero value, so mergeStruct would silently
	// restore the old (non-zero) level. We treat an explicit NoAccess from a
	// System that has a SystemImpl as a meaningful "revoke access" signal.
	if hasIncomingAccessLevel && newAccessLevel == NoAccess {
		if sys, ok := new.(System); ok && sys.GetAccessLevel() != NoAccess {
			sys.SetAccessLevel(NoAccess)
			new = sys.(Entity)
		}
	}

	return new
}

func mergeObjects(new, old interface{}) (interface{}, error) {
	newVal := reflect.ValueOf(new)
	oldVal := reflect.ValueOf(old)

	if newVal.Kind() == reflect.Ptr {
		newVal = newVal.Elem()
	}
	if oldVal.Kind() == reflect.Ptr {
		oldVal = oldVal.Elem()
	}

	if newVal.Type() != oldVal.Type() {
		return nil, fmt.Errorf("cannot merge entities of different types")
	}

	merged := reflect.New(newVal.Type()).Elem()

	for i := range newVal.NumField() {
		structField := newVal.Type().Field(i)
		if !structField.IsExported() {
			continue
		}

		newField := newVal.Field(i)
		oldField := oldVal.Field(i)

		switch {
		case structField.Anonymous && newField.Kind() == reflect.Struct:
			merged.Field(i).Set(mergeStruct(newField, oldField))

		case newField.Kind() == reflect.Struct:
			merged.Field(i).Set(mergeStruct(newField, oldField))

		default:
			merged.Field(i).Set(mergeValue(newField, oldField))
		}
	}

	return merged.Interface(), nil
}

func mergeEntities(newEntity, oldEntity Entity) Entity {
	merged, err := mergeObjects(newEntity, oldEntity)
	// happy path: if they could be merged without problems, no further action is needed
	if err == nil {
		return merged.(Entity)
	}

	newSys, newIsSys := newEntity.(System)
	oldSys, oldIsSys := oldEntity.(System)
	if newIsSys && oldIsSys {
		sys, err := mergeSystems(newSys, oldSys)
		if err == nil {
			return sys
		}
	}
	slog.Error(fmt.Sprintf("Can't merge entities of types %v and %v", newEntity, oldEntity))
	return nil
}

func _mergeSystemCaster(new, old interface{}, t reflect.Type) (System, error) {
	res, err := mergeObjects(new, old)
	if err != nil {
		return nil, err
	}
	mergedVal := reflect.ValueOf(res)
	if mergedVal.Type() != t {
		// If mergeObjects returns a pointer, dereference if needed
		if mergedVal.Kind() == reflect.Ptr && mergedVal.Elem().Type() == t {
			mergedVal = mergedVal.Elem()
		}
	}
	// Convert to correct type
	return mergedVal.Interface().(System), nil
}

// Merge two systems of the same type, or promote an UnknownSystem to the type of the other system if one of them is unknown.
// If both systems are of different types and neither is unknown, an error is returned.
// The merging logic is as follows: if one of the systems has a zero value for a field and the other has a non-zero value, the non-zero value is used.
// If both have non-zero values, the new system's value is used.
// If both have zero values, the result is zero (already set).
func mergeSystems(new, old System) (System, error) {
	if new == nil {
		slog.Warn("Attempt to merge with nil system", "a", new, "b", old)
		return old, nil
	} else if old == nil {
		slog.Warn("Attempt to merge with nil system", "a", new, "b", old)
		return new, nil
	}

	aVal := reflect.ValueOf(new)
	bVal := reflect.ValueOf(old)
	aType := aVal.Type()
	bType := bVal.Type()

	// if it's the same types, then a regular object merge works
	if aType == bType {
		return _mergeSystemCaster(new, old, aType)
	}

	// special handling for divergent types: only UnknownSystem may be promoted
	// other types can't be merged and yield an error

	var unknownSystem UnknownSystem
	var promoSys System

	if u, ok := new.(UnknownSystem); ok {
		unknownSystem = u
		promoSys = old
	} else if u, ok = old.(UnknownSystem); ok {
		unknownSystem = u
		promoSys = new
		// continue with UnknownSystem promotion logic below
	} else { // no UnknownSystem provided, can't promote sibling types
		return nil, fmt.Errorf("cannot merge systems of different types: %T vs %T", new, old)
	}

	switch p := promoSys.(type) {
	case Pod:
		tmp, err := unknownSystem.PromoteToPod()
		if err != nil {
			return nil, fmt.Errorf("cannot promote UnknownSystem to Pod: %w", err)
		}
		return _mergeSystemCaster(promoSys, tmp, reflect.TypeOf(tmp))
	case K8sNode:
		tmp, err := unknownSystem.PromoteToK8sNode()
		if err != nil {
			return nil, fmt.Errorf("cannot promote UnknownSystem to K8sNode: %w", err)
		}
		return _mergeSystemCaster(promoSys, tmp, reflect.TypeOf(tmp))
	default:
		return nil, fmt.Errorf("Promotion of UnknownSystem to type %T is not yet supported", p)
	}
}

func mergeValue(newField, oldField reflect.Value) reflect.Value {
	if !newField.IsValid() {
		return oldField
	}
	if !oldField.IsValid() {
		return newField
	}

	switch newField.Kind() {
	case reflect.Struct:
		return mergeStruct(newField, oldField)
	case reflect.Ptr:
		if newField.IsNil() && !oldField.IsNil() {
			return oldField
		} else if !newField.IsNil() && oldField.IsNil() {
			return newField
		} else if !newField.IsNil() && !oldField.IsNil() {
			merged := mergeValue(newField.Elem(), oldField.Elem())
			ptr := reflect.New(merged.Type())
			ptr.Elem().Set(merged)
			return ptr
		}
		return newField
	case reflect.Slice:
		if newField.Len() == 0 && oldField.Len() > 0 {
			return oldField
		}
		if oldField.Len() == 0 {
			return newField
		}
		return mergeSlices(newField, oldField)
	case reflect.Map:
		if newField.Len() == 0 && oldField.Len() > 0 {
			return oldField
		}
		if oldField.Len() == 0 {
			return newField
		}
		return mergeMaps(newField, oldField)
	default:
		if isZeroValue(newField) {
			return oldField
		}
		return newField
	}
}

// mergeSlices appends items from oldSlice that are not already present in
// newSlice, using reflect.DeepEqual for comparison.
func mergeSlices(newSlice, oldSlice reflect.Value) reflect.Value {
	merged := reflect.MakeSlice(newSlice.Type(), newSlice.Len(), newSlice.Len()+oldSlice.Len())
	reflect.Copy(merged, newSlice)

	for i := 0; i < oldSlice.Len(); i++ {
		oldItem := oldSlice.Index(i)
		found := false
		for j := 0; j < newSlice.Len(); j++ {
			if reflect.DeepEqual(oldItem.Interface(), newSlice.Index(j).Interface()) {
				found = true
				break
			}
		}
		if !found {
			merged = reflect.Append(merged, oldItem)
		}
	}
	return merged
}

// mergeMaps merges two maps, with new values taking precedence for duplicate keys.
func mergeMaps(newMap, oldMap reflect.Value) reflect.Value {
	merged := reflect.MakeMap(newMap.Type())
	for _, key := range oldMap.MapKeys() {
		merged.SetMapIndex(key, oldMap.MapIndex(key))
	}
	for _, key := range newMap.MapKeys() {
		merged.SetMapIndex(key, newMap.MapIndex(key))
	}
	return merged
}

func mergeStruct(newVal, oldVal reflect.Value) reflect.Value {
	merged := reflect.New(newVal.Type()).Elem()

	for i := 0; i < newVal.NumField(); i++ {
		structField := newVal.Type().Field(i)
		if !structField.IsExported() {
			continue
		}

		nf := newVal.Field(i)
		of := oldVal.Field(i)

		switch {
		case structField.Anonymous && nf.Kind() == reflect.Struct:
			// Embedded struct: recurse and set fields individually
			embedded := mergeStruct(nf, of)
			merged.Field(i).Set(embedded)

		case nf.Kind() == reflect.Struct:
			merged.Field(i).Set(mergeStruct(nf, of))

		default:
			merged.Field(i).Set(mergeValue(nf, of))
		}
	}

	return merged
}

func isZeroValue(v reflect.Value) bool {
	return reflect.DeepEqual(v.Interface(), reflect.Zero(v.Type()).Interface())
}
