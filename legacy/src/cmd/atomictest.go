package cmd

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"strings"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/core"
	"github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
	"github.com/jedib0t/go-pretty/v6/table"
	"github.com/spf13/cobra"
)

func newAtomicTestCmd(rootCmd *cobra.Command) *cobra.Command {
	var ttpID string
	var target string
	var kubeconfigPath string
	cmd := &cobra.Command{
		Use:   "invoke [ttpID]",
		Short: "Run an atomic test in a Kubernetes cluster",
		Args:  cobra.MinimumNArgs(1),
		Run: func(cmd *cobra.Command, args []string) {
			ttpID = args[0]
			ran := core.InitRan(target, "../armory/", kubeconfigPath)
			ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
			defer cancel()
			err := ran.ExecuteAtomicTTP(ctx, ttpID, target)
			if err != nil {
				fmt.Println("❌", err.Error())
			}
		},
	}

	cmd.ValidArgsFunction = func(cmd *cobra.Command, args []string, toComplete string) ([]string, cobra.ShellCompDirective) {
		suggestions := []string{}
		armoryDir := "../armory/"
		a := armory.Armory{SrcDir: armoryDir}
		err := a.Load()
		if err != nil {
			fmt.Println("Error loading armory:", err.Error())
		} else {
			ttps := a.GetTTPs()
			for _, ttp := range ttps {
				if strings.HasPrefix(ttp.GetID(), toComplete) || strings.HasPrefix(ttp.GetTitle(), toComplete) {
					suggestions = append(suggestions, ttp.GetID())
				}
			}
		}

		return suggestions, cobra.ShellCompDirectiveNoFileComp
	}

	cmd.Flags().StringVarP(&target, "target", "t", "", `set the initial target for the emulation. In the pattern "<ns>/<service or pod>" or a URL`)
	cmd.Flags().StringVar(&kubeconfigPath, "kubeconfig", "", "path to the kubeconfig file (default: $HOME/.kube/config)")
	cmd.RegisterFlagCompletionFunc("target", func(cmd *cobra.Command, args []string, toComplete string) ([]string, cobra.ShellCompDirective) {
		// Dynamically compute suggestions based on toComplete or context
		podIDs, err := k8s.GetIDsOfRunningPods(context.Background(), "")
		if err != nil {
			fmt.Fprintln(os.Stderr, "Error getting running pods:", err)
		}
		return podIDs, cobra.ShellCompDirectiveNoFileComp
	})
	return cmd
}

func newShowArmoryCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "armory",
		Short: "Show the contents of the armory",
		Run: func(cmd *cobra.Command, args []string) {
			ttps, err := getTTPs()
			if err != nil {
				fmt.Println("Error loading armory:", err.Error())
			} else {
				printTTPs(ttps)
			}
		},
	}
	return cmd
}

func printTTPs(ttps []domain.TTP) {
	t := table.NewWriter()
	t.AppendHeader(table.Row{"TTP ID", "Name", "Tactic", "Status", "Description"})
	t.SetOutputMirror(os.Stdout)
	t.SetStyle(table.StyleLight)
	for _, ttp := range ttps {
		t.AppendRow(table.Row{ttp.GetID(), ttp.GetTitle(), ttp.Tactic, ttp.Status, ttp.GetDescription()})
	}
	t.Render()
}

func getTTPs() ([]domain.TTP, error) {
	armoryDir := "../armory/"
	a := armory.Armory{SrcDir: armoryDir}
	err := a.Load()
	if err != nil {
		return nil, fmt.Errorf("error loading armory: %w", err)
	}
	return a.GetTTPs(), nil
}
