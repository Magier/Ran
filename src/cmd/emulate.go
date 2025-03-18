package cmd

import (
	"github.com/Magier/Ran/core"
	"github.com/Magier/Ran/tui"
	"github.com/spf13/cobra"
)

func newEmulationCmd() *cobra.Command {
	var target string
	var godMode bool
	var planPath string
	cmd := &cobra.Command{
		Use:   "emulate",
		Short: "Emulate adversary behavior against a Kubernetes cluster",
		Run: func(cmd *cobra.Command, args []string) {
			ran := core.InitRan(target)
			t := tui.SetupTUI(ran)
			ran.Start(godMode, planPath)
			tui.RunTUI(t)
		},
	}
	cmd.Flags().BoolVar(&godMode, "godmode", false, "enable Godmode to use the local kubeconfig context to load all available resources")
	cmd.Flags().StringVarP(&target, "target", "t", "", `set the initial target for the emulation. In the pattern "<ns>/<service or pod>" or a URL`)
	cmd.Flags().StringVarP(&planPath, "path", "p", "", `path to the file of the plan`)
	return cmd
}
