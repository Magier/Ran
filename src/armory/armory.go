package armory

import (
	"cmp"
	"embed"
	"errors"
	"fmt"
	"io/fs"
	"log/slog"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/mitre"
	"gopkg.in/yaml.v3"
)

//go:embed all:builtin/*
var builtinFS embed.FS

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
	a.ttps, err = loadTTPs(builtinFS, a.SrcDir)
	if err != nil {
		return errors.New("Couldn't load armory: " + err.Error())
	}

	// TODO: re-enabled loading TTPs from tools
	// toolTTPs, err := loadTools(filepath.Join(a.SrcDir, "tools"))
	// if err != nil {
	// 	return errors.New("Couldn't load tools: " + err.Error())
	// }
	// a.ttps = append(a.ttps, toolTTPs...)
	if len(a.ttps) == 0 {
		return errors.New("Neither builtin nor user-defined TTPs loaded")
	}
	return nil
}

// helper function to abstract the file system walking for regular and embedded FS
// root is a slash-separated path *inside* fsys (e.g. ".", "assets", "templates").
// visit is called for every non-directory. Return fs.SkipDir to prune a dir.
func walkFS(fsys fs.FS, root string, visit func(path string, content []byte) error) error {
	return fs.WalkDir(fsys, root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		f, err := fs.ReadFile(fsys, path)
		if err != nil {
			return fmt.Errorf("failed to read file %s: %w", path, err)
		}
		return visit(path, f)
	})
}

func loadTTPs(builtinFS embed.FS, userDefinedDir string) ([]domain.TTP, error) {
	ttps := []domain.TTP{}

	visitFn := func(path string, content []byte) error {
		if !strings.HasSuffix(path, ".yaml") || strings.HasSuffix(path, "dummy.yaml") {
			return nil
		}

		var ttp domain.TTP
		if err := yaml.Unmarshal(content, &ttp); err != nil {
			return fmt.Errorf("failed to unmarshal YAML content from file %s: %w", path, err)
		}
		if ttp.Status == "" {
			ttp.Status = "enabled"
		}
		ttps = append(ttps, ttp)
		return nil
	}

	// read just the TTPs from the embedded filesystem
	if err := walkFS(builtinFS, ".", visitFn); err != nil {
		return ttps, errors.New("Couldn't load builtin TTPs: " + err.Error())
	}
	// read the user-defined TTPs
	if userDefinedDir != "" {
		userDefinedFS := os.DirFS(userDefinedDir)
		if _, err := os.Stat(userDefinedDir); os.IsNotExist(err) {
			slog.Warn(fmt.Sprintf("User-defined TTP directory '%s' does not exist, skipping\n", userDefinedDir))
			return ttps, nil
		}

		if err := walkFS(userDefinedFS, ".", visitFn); err != nil {
			return ttps, errors.New("Couldn't load TTPs: " + err.Error())
		}
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
