package domain

import "fmt"

type Command interface {
	Message
	String() string
}

type StartListener struct {
	Port uint
}

func (c StartListener) MessageName() string {
	return "StartListener"
}
func (c StartListener) String() string {
	return fmt.Sprintf("Listener on port %d started", c.Port)
}

type StartC2Redirector struct {
	DstPort uint
}

func (c StartC2Redirector) MessageName() string {
	return "StartC2Redirector"
}
func (c StartC2Redirector) String() string {
	return "Started C2 redirector"
}

type ReadEnvVars struct {
	Target          string
	TargetNamespace string
	TargetId        string
}

func (c ReadEnvVars) MessageName() string {
	return "ReadEnvVars"
}
func (c ReadEnvVars) String() string {
	return "Read environment variables"
}

type ExecCmd struct {
	Cmd        string
	Args       []string
	TargetId   string
	TargetName string
	TargetNs   string
}

func (e ExecCmd) MessageName() string {
	return "ExecCmd"
}

func (e ExecCmd) String() string {
	return fmt.Sprintf("Executed %s on %s/%s", e.Cmd, e.TargetNs, e.TargetName)
}
