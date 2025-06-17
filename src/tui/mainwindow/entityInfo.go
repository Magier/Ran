package mainwindow

import (
	"fmt"
	"strings"

	"github.com/Magier/Ran/domain"
	"github.com/charmbracelet/lipgloss"
)

func strOrDefault(s, def string) string {
	if s == "" {
		return def
	}
	return s
}

func renderEntity(entity domain.Entity) string {
	switch e := entity.(type) {
	case domain.Pod:
		return renderPod(e)
	case domain.ServiceAccount:
		return renderServiceAccount(e)
	case domain.C2System:
		return renderC2(e)
	case domain.Namespace:
		return renderNamespace(e)
	case domain.Service:
		return renderService(e)
	case nil:
		return "-"
	}
	return entity.GetId()
}

func renderPod(e domain.Pod) string {
	ips := []string{}
	for _, ip := range e.IPs {
		ips = append(ips, ip.String())
	}

	lines := []string{
		"Pod ID: " + e.GetId(),
		"Namespace: " + e.Namespace,
		"HostName:" + strOrDefault(e.HostName, "?"),
		"IP: " + strOrDefault(strings.Join(ips, ", "), "?"),
		"NodeName: " + strOrDefault(e.NodeName, "?"),
		"HostPID: " + strOrDefault(e.HostPID.String(), "?"),
		"HostIPC: " + strOrDefault(e.HostIPC.String(), "?"),
		"HostNetwork: " + strOrDefault(e.HostNetwork.String(), "?"),
		"AccessLevel: " + e.AccessLevel.String(),
	}

	// e.EnvVars

	// e.mounts

	// e.bins

	return lipgloss.JoinVertical(lipgloss.Left, lines...)
}

func renderC2(c2 domain.C2System) string {
	return "C2: " + c2.Name
}
func renderNamespace(ns domain.Namespace) string {
	lines := []string{
		ns.GetKind() + ": " + ns.Name,
		"PSA: ?",
	}

	return lipgloss.JoinVertical(lipgloss.Left, lines...)
}

func renderServiceAccount(sa domain.ServiceAccount) string {
	lines := []string{
		sa.GetKind() + ": " + sa.Name,
		"ID: " + sa.GetId(),
		"Node: " + sa.Token.Kubernetes.Node.Name,
	}

	if len(sa.Can) > 0 {
		lines = append(lines, "Can: ")
		for _, rule := range sa.Can {
			lines = append(lines, fmt.Sprintf("\t - %s: %s", rule.Verb, rule.ResourceType))
		}
	}

	return lipgloss.JoinVertical(lipgloss.Left, lines...)
}

func renderService(svc domain.Service) string {
	portInfos := []string{}
	for name, p := range svc.Ports {
		portInfos = append(portInfos, fmt.Sprintf("%s:%d", name, p))
	}

	lines := []string{
		svc.GetKind() + ": " + svc.Name,
		"ID: " + svc.GetId(),
		"Ports: " + strings.Join(portInfos, ", "),
		"Targets: " + strings.Join(svc.Targets, ", "),
	}

	return lipgloss.JoinVertical(lipgloss.Left, lines...)
}
