package domain

import "strings"

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
	case "role":
		return "r"
	case "clusterrolebinding":
		return "crb"
	case "clusterrole":
		return "cr"
	default:
		return k
	}
}
