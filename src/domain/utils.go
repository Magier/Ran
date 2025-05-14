package domain

import (
	"reflect"
	"strings"
	"unicode"
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

	if ownable, ok := new.(Ownable); ok {
		hasOwner := false
		ownerRef, _ := ownable.GetOwner()
		if ownerRef.Name == "" {
			prevOwnable := old.(Ownable)
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

func mergeEntities(newEntity, oldEntity Entity) Entity {
	newVal := reflect.ValueOf(newEntity)
	oldVal := reflect.ValueOf(oldEntity)

	if newVal.Kind() == reflect.Ptr {
		newVal = newVal.Elem()
	}
	if oldVal.Kind() == reflect.Ptr {
		oldVal = oldVal.Elem()
	}

	if newVal.Type() != oldVal.Type() {
		panic("cannot merge entities of different types")
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

	return merged.Interface().(Entity)
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
