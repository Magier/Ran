package core

import (
	"context"
	"fmt"
	"os"
	"os/signal"

	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/internal/bus"
	k8s "github.com/Magier/Ran/internal/k8sclient"
	"github.com/Magier/Ran/planner"
	"github.com/Magier/Ran/tui"
	tea "github.com/charmbracelet/bubbletea"
)

func StartRan(withTui bool, loadKubeConfig bool) {
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

	if loadKubeConfig {
		go func() {
			channel := make(chan domain.Pod)
			go populatePods(ctx, channel)
			fmt.Println("Populating pods")
			for p := range channel {
				err := mb.Publish(domain.NewEntity{Pod: p})
				if err != nil {
					fmt.Printf("Couldn't publish newEntity event: %s", err.Error())
				}
			}
		}()
	}

	if ui != nil {
		tui.RunTUI(ui)
	}
}

func populatePods(ctx context.Context, channel chan<- domain.Pod) {
	defer close(channel)
	client, err := k8s.NewK8sClient("")
	if err != nil {
		panic(err)
	}
	pods, err := k8s.GetPods(ctx, client)
	if err != nil {
		panic(err)
	}
	for _, p := range pods {
		channel <- p
	}

}
