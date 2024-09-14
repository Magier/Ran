package core

import (
	"context"
	"fmt"
	"net"
	"os"
	"os/signal"
	"strings"

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
		go loadClusterData(ctx, mb)
	}

	if ui != nil {
		tui.RunTUI(ui)
	}
}

func loadClusterData(ctx context.Context, mb bus.MessageBus) {
	channel := make(chan domain.Pod)

	k8sConfig, err := k8s.GetConfig()
	if err != nil {
		panic(err)
	}
	// extract the HOST of the url which is an ip address
	apiServerIP := strings.Split(k8sConfig.Host, ":")[1][2:]
	apiServerIPAddr, err := net.ResolveIPAddr("ip", apiServerIP)
	if err != nil {
		fmt.Printf("Couldn't resolve apiServer IP: %s", err.Error())
	}

	apiServerPod := domain.ApiServer{
		Pod: domain.Pod{
			Name:      "*API Server",
			Namespace: "kube-system",
		},
		ExternalIP: *apiServerIPAddr,
		CAData:     k8sConfig.CAData,
	}
	k8sConfigUser := domain.Identity{
		Name:     k8sConfig.Username,
		CertData: k8sConfig.CertData,
		KeyData:  k8sConfig.KeyData,
	}
	err = mb.Publish(domain.NewEntities{
		Pods:       []domain.PodInterface{apiServerPod},
		Identities: []domain.Identity{k8sConfigUser}})
	if err != nil {
		fmt.Printf("Couldn't add apiServer as new entity to bus: %s", err.Error())
	}

	go populatePods(ctx, channel)
	pods := []domain.PodInterface{}
	for p := range channel {
		pods = append(pods, p)
	}
	err = mb.Publish(domain.NewEntities{Pods: pods})
	if err != nil {
		fmt.Printf("Couldn't publish newEntity event: %s", err.Error())
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
