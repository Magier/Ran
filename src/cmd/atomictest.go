package cmd

import (
	"context"
	"fmt"
	"os"
	"os/signal"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/core"
	"github.com/jedib0t/go-pretty/v6/table"
	"github.com/spf13/cobra"
)

func newAtomicTestCmd() *cobra.Command {
	var ttpID string
	var target string
	cmd := &cobra.Command{
		Use:   "invoke",
		Short: "Run an atomic test in a Kubernetes cluster",
		Args:  cobra.MinimumNArgs(1),
		Run: func(cmd *cobra.Command, args []string) {
			ttpID = args[0]
			ran := core.InitRan(target, "../armory/")
			ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
			defer cancel()
			err := ran.ExecuteAtomicTTP(ctx, ttpID, target)
			if err != nil {
				fmt.Println("Error executing atomic TTP:", err.Error())
			}
		},
	}
	cmd.Flags().StringVarP(&target, "target", "t", "", `set the initial target for the emulation. In the pattern "<ns>/<service or pod>" or a URL`)
	return cmd
}

func newShowArmoryCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "armory",
		Short: "Show the contents of the armory",
		Run: func(cmd *cobra.Command, args []string) {
			armoryDir := "../armory/"
			a := armory.Armory{SrcDir: armoryDir}
			err := a.Load()
			if err != nil {
				fmt.Println("Error loading armory:", err.Error())
			} else {
				t := table.NewWriter()
				t.AppendHeader(table.Row{"TTP ID", "Name", "Tactic", "Description"})
				t.SetOutputMirror(os.Stdout)
				t.SetStyle(table.StyleLight)
				for _, ttp := range a.GetTTPs() {
					t.AppendRow(table.Row{ttp.GetID(), ttp.GetTitle(), ttp.Tactic, ttp.GetDescription()})
				}
				t.Render()
			}
		},
	}
	return cmd
}
