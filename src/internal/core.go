package core

import (
	"context"
	"os"
	"os/signal"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/internal/bus"
	k8s "github.com/Magier/Ran/internal/k8sclient"
	"github.com/Magier/Ran/planner"
	"github.com/Magier/Ran/tui"
	tea "github.com/charmbracelet/bubbletea"
)

func StartRan(withTui bool, loadKubeConfig bool) {
	if loadKubeConfig {
		client, err := k8s.NewK8sClient("")
		if err != nil {
			panic(err)
		}
		_ = client
	}

	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	// ctx, cancel := context.WithCancel(context.Background(), os.Interrupt)
	defer cancel()
	mb := bus.CreateMessageBus()
	c := campaign.StartCampaign(mb)
	var ui *tea.Program = nil
	if withTui {
		ui = tui.SetupTUI(mb, c)
	}
	c2.StartC2(ctx, mb)
	planner.StartApi(mb)

	go mb.HandleEvents(ctx)
	// TODO maybe switch between TUI and web-UI (start frontend as well?)
	if ui != nil {
		tui.RunTUI(ui)
	}
}
