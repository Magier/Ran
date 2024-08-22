package cmd

import (
	core "github.com/Magier/Ran/internal"
	"github.com/spf13/cobra"
)

func newAtomicTestCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "test",
		Short: "Run an atomic test in a Kubernetes cluster",
		Run: func(cmd *cobra.Command, args []string) {
			core.StartRan(true, true)
		},
	}
}
