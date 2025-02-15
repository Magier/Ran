package cmd

import (
	core "github.com/Magier/Ran/internal"
	"github.com/spf13/cobra"
)

func newAtomicTestCmd() *cobra.Command {
	var godMode bool
	cmd := &cobra.Command{
		Use:   "test",
		Short: "Run an atomic test in a Kubernetes cluster",
		Run: func(cmd *cobra.Command, args []string) {
			core.StartRan(true, godMode)
		},
	}

	cmd.Flags().BoolVar(&godMode, "godmode", false, "enable Godmode to use the local kubeconfig context to load all available resources")
	cmd.Flags().StringP("target", "t", "", `set the initial target for the emulation. In the pattern "<ns>/<service or pod>" or a URL`)
	return cmd
}
