package cmd

import (
	"context"
	"fmt"
	"os"
	"os/signal"

	"github.com/Magier/Ran/api"
	"github.com/Magier/Ran/core"
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
			ran := core.InitRan(target, "../armory/")
			ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
			api := api.NewAPI(&ran, ctx)
			// t := tui.SetupTUI(ran)
			defer cancel()
			err := ran.Start(ctx, godMode, planPath)
			if err != nil {
				fmt.Println("❌ failed to start Ran", err.Error())
			} else {
				if err = api.StartServer(":8080"); err != nil {
					fmt.Println("❌ failed to start API server", err.Error())
				}
				fmt.Printf("post start")
				// tui.RunTUI(t)
			}
		},
	}
	cmd.Flags().BoolVar(&godMode, "godmode", false, "enable Godmode to use the local kubeconfig context to load all available resources")
	cmd.Flags().StringVarP(&target, "target", "t", "", `set the initial target for the emulation. In the pattern "<ns>/<service or pod>" or a URL`)
	cmd.Flags().StringVarP(&planPath, "path", "p", "", `path to the file of the plan`)
	return cmd
}
