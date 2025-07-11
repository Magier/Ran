package cmd

import (
	"context"
	"os"
	"os/signal"

	"github.com/Magier/Ran/core"
	"github.com/spf13/cobra"
)

func newAtomicTestCmd() *cobra.Command {
	var ttpID string
	var target string
	cmd := &cobra.Command{
		Use:   "invoke",
		Short: "Run an atomic test in a Kubernetes cluster",
		Args:  cobra.ArbitraryArgs,
		Run: func(cmd *cobra.Command, args []string) {
			ttpID = args[0]
			ran := core.InitRan(target, "../armory/")
			ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
			defer cancel()
			ran.ExecuteAtomicTTP(ctx, ttpID, target)
		},
	}
	cmd.Flags().StringVarP(&target, "target", "t", "", `set the initial target for the emulation. In the pattern "<ns>/<service or pod>" or a URL`)
	return cmd
}
