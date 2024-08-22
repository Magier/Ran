package cmd

import (
	"fmt"

	"github.com/spf13/cobra"
)

func newEmulationCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "emulate",
		Short: "Emulate adversary behavior against a Kubernetes cluster",
		Run: func(cmd *cobra.Command, args []string) {
			// Do Stuff Here
			fmt.Println("Running emulation")
		},
	}
}
