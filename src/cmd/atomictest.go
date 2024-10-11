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
	return cmd
}
