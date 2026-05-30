package cmd

import (
	"fmt"
	"log/slog"
	"os"

	"github.com/Magier/Ran/config"
	k8s "github.com/Magier/Ran/k8sclient"
	"github.com/spf13/cobra"
)

var cfgFile string

func init() {
	cobra.OnInitialize(initConfig)
}

func initConfig() {
	// Load the Ran config once at startup
	cfg, err := config.Load(cfgFile)
	if err != nil {
		slog.Warn("Failed to load Ran config, using defaults", "error", err)
		// Use default config
		cfg, _ = config.Load(cfgFile) // This will return default config
	}

	// Set the namespace filter for k8s client components to use
	k8s.SetNamespaceFilter(k8s.NamespaceFilter{
		Excluded: cfg.Namespaces.Excluded,
		Included: cfg.Namespaces.Included,
	})
}

func Execute() {
	var rootCmd = &cobra.Command{
		Use:   "ran",
		Short: "Ran is an adversary emulation tool for Kubernetes",
		Long:  ``,
		RunE: func(cmd *cobra.Command, args []string) error {
			return cmd.Help()
		},
	}

	// Global flag for config file path
	rootCmd.PersistentFlags().StringVar(&cfgFile, "config", "", "config file (default is ran.yaml)")

	rootCmd.AddCommand(newAtomicTestCmd(rootCmd))
	rootCmd.AddCommand(newEmulationCmd())
	rootCmd.AddCommand(newShowArmoryCmd())

	if err := rootCmd.Execute(); err != nil {
		fmt.Println(err)
		os.Exit(1)
	}
}
