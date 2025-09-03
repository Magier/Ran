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

func _mergeSystemCaster(a, b interface{}, t reflect.Type) (System, error) {
	res, err := mergeObjects(a, b)
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

func mergeSystems(a, b System) (System, error) {
	if a == nil {
		slog.Warn("Attempt to merge with nil system", "a", a, "b", b)
		return b, nil
	} else if b == nil {
		slog.Warn("Attempt to merge with nil system", "a", a, "b", b)
		return a, nil
	}

	aVal := reflect.ValueOf(a)
	bVal := reflect.ValueOf(b)
	aType := aVal.Type()
	bType := bVal.Type()

	// if it's the same types, then a regular object merge works
	if aType == bType {
		return _mergeSystemCaster(a, b, aType)
	}

	// special handling for divergent types: only UnknownSystem may be promoted
	// other types can't be merged and yield an error

	var unknownSystem UnknownSystem
	var promoSys System

	if u, ok := a.(UnknownSystem); ok {
		unknownSystem = u
		promoSys = b
	} else if u, ok = b.(UnknownSystem); ok {
		unknownSystem = u
		promoSys = a
		// continue with UnknownSystem promotion logic below
	} else { // no UnknownSystem provided, can't promote sibling types
		return nil, fmt.Errorf("cannot merge systems of different types: %T vs %T", a, b)
	}

	switch p := promoSys.(type) {
	case Pod:
		tmp, err := unknownSystem.PromoteToPod()
		if err != nil {
			return nil, fmt.Errorf("cannot promote UnknownSystem to Pod: %w", err)
		}
		return _mergeSystemCaster(tmp, promoSys, reflect.TypeOf(tmp))
	case K8sNode:
		tmp, err := unknownSystem.PromoteToK8sNode()
		if err != nil {
			return nil, fmt.Errorf("cannot promote UnknownSystem to K8sNode: %w", err)
		}
		return _mergeSystemCaster(tmp, promoSys, reflect.TypeOf(tmp))
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
		return newField
	case reflect.Map:
		if newField.Len() == 0 && oldField.Len() > 0 {
			return oldField
		}
		return newField
	default:
		if isZeroValue(newField) {
			return oldField
		}
		return newField
	}
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
