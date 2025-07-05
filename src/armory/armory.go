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
	"github.com/Magier/Ran/mitre"
	"gopkg.in/yaml.v3"
)

type Armory struct {
	SrcDir string
	ttps   []domain.TTP
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

func (a *Armory) Load() error {
	var err error
	a.ttps, err = loadTTPs(filepath.Join(a.SrcDir, "ttps"))
	if err != nil {
		return errors.New("Couldn't load armory: " + err.Error())
	}

	// TODO: re-enabled loading TTPs from tools
	// toolTTPs, err := loadTools(filepath.Join(a.SrcDir, "tools"))
	// if err != nil {
	// 	return errors.New("Couldn't load tools: " + err.Error())
	// }
	// a.ttps = append(a.ttps, toolTTPs...)

	return nil
}

func loadTTPs(dir string) ([]domain.TTP, error) {
	ttps := []domain.TTP{}

	// parse "attacks as code" in the specified dir folder
	walkFn := func(w string, d fs.DirEntry, err error) error {
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
			// ttp.CommandMsg = parseCommandToMessage(ttp.Command)
			ttps = append(ttps, ttp)
		}
		// if strings.HasSuffix(w, ".md") {
		// }
		return nil
	}

	if err := filepath.WalkDir(dir, walkFn); err != nil {
		return ttps, errors.New("Couldn't load TTPs: " + err.Error())
	}
	return sortTTPs(ttps), nil
}

// order all the TTPs first by the Tactic and then by their names
func sortTTPs(ttps []domain.TTP) []domain.TTP {
	tacticOrder := []mitre.Tactic{
		mitre.Reconnaissance,
		mitre.ResourceDevelopment,
		mitre.InitialAccess,
		mitre.Discovery,
		mitre.Execution,
		mitre.CredentialAccess,
		mitre.Persistence,
		mitre.PrivilegeEscalation,
		mitre.DefenseEvasion,
		mitre.LateralMovement,
		mitre.Collection,
		mitre.CommandAndControl,
		mitre.Exfiltration,
		mitre.Impact,
	}

	tacticIndex := make(map[mitre.Tactic]int)
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

func loadTools(dir string) ([]domain.TTP, error) {
	ttps := []domain.TTP{}

	err := filepath.WalkDir(dir, func(w string, d fs.DirEntry, err error) error {
		if d.IsDir() {
			return err
		}

		if strings.HasSuffix(w, ".yaml") {
			content, err := os.ReadFile(w)
			if err != nil {
				return fmt.Errorf("failed to read file %s: %w", w, err)
			}

			var tool domain.Tool
			err = yaml.Unmarshal(content, &tool)
			if err != nil {
				return fmt.Errorf("failed to unmarshal YAML content from file %s: %w", w, err)
			}

			for _, ttp := range tool.TTPs {
				ttp.Name = fmt.Sprintf("%s: %s", tool.Name, ttp.Name)
				// ttp.CommandMsg = parseCommandToMessage(ttp.Command)
				ttps = append(ttps, ttp)
			}
		}
		return nil
	})

	if err != nil {
		return ttps, errors.New("Couldn't load TTPs: " + err.Error())
	}
	return ttps, nil
}
