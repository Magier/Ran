package domain

import (
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
