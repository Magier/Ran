package cmd

import (
	"context"
	"os"
	"os/signal"

	"github.com/Magier/Ran/core"
	"github.com/Magier/Ran/tui"
	"github.com/spf13/cobra"
)

func newAtomicTestCmd() *cobra.Command {
	var godMode bool
	var target string
	cmd := &cobra.Command{
		Use:   "invoke",
		Short: "Run an atomic test in a Kubernetes cluster",
		Run: func(cmd *cobra.Command, args []string) {
			ran := core.InitRan(target, "../armory/", "../sliver_cfg.json")
			t := tui.SetupTUI(ran)
			ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
			defer cancel()
			ran.Start(ctx, godMode, "")
			tui.RunTUI(t)
		},
	}

	cmd.Flags().BoolVar(&godMode, "godmode", false, "enable Godmode to use the local kubeconfig context to load all available resources")
	cmd.Flags().StringVarP(&target, "target", "t", "", `set the initial target for the emulation. In the pattern "<ns>/<service or pod>" or a URL`)
	return cmd
}
