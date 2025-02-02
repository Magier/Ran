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
