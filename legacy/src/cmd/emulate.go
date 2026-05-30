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
	var armoryPath string
	var kubeconfigPath string
	var port int
	cmd := &cobra.Command{
		Use:   "emulate",
		Short: "Emulate adversary behavior against a Kubernetes cluster",
		Run: func(cmd *cobra.Command, args []string) {
			ran := core.InitRan(target, armoryPath, kubeconfigPath)
			ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
			api := api.NewAPI(&ran, ctx)
			// t := tui.SetupTUI(ran)
			defer cancel()
			err := ran.Start(ctx, godMode, planPath)
			if err != nil {
				fmt.Println("❌ failed to start Ran", err.Error())
				return
			}
			fmt.Println("🚀 Server started on", fmt.Sprintf(":%d", port))
			fmt.Println("Press CTRL+C to stop")

			// StartServer blocks until context is cancelled
			if err := api.StartServer(ctx, fmt.Sprintf(":%d", port)); err != nil {
				fmt.Println("❌ Server error:", err)
			}
		},
	}
	cmd.Flags().BoolVar(&godMode, "godmode", false, "enable Godmode to use the local kubeconfig context to load all available resources")
	cmd.Flags().StringVarP(&target, "target", "t", "", `set the initial target for the emulation. In the pattern "<ns>/<service or pod>" or a URL`)
	cmd.Flags().StringVarP(&planPath, "path", "f", "", `path to the file of the plan`)
	cmd.Flags().IntVarP(&port, "port", "p", 8080, "port to run the server on")
	cmd.Flags().StringVarP(&armoryPath, "armory", "a", "", `path to the armory containing TTPs`)
	cmd.Flags().StringVar(&kubeconfigPath, "kubeconfig", "", "path to the kubeconfig file (default: $HOME/.kube/config)")

	return cmd
}
