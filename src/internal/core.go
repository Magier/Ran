package core

import (
	"context"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"strings"

	"github.com/Magier/Ran/armory"
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
	a, err := armory.LoadArmory()
	if err != nil {
		panic(err)
	}
	c := campaign.StartCampaign(mb)
	var ui *tea.Program = nil
	if withTui {
		ui = tui.SetupTUI(mb, c, a)
	}

	// TODO: turn fileshare into a regular action
	filesharePort, _ := c.GetFileshare()
	go ServeFiles(ctx, filesharePort)
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
	channel := make(chan domain.Entity)

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
			K8sEntity: domain.K8sEntity{
				Id:   "#apiServer",
				Name: "#API Server",
				Kind: "Pod",
			},
			NamespacedResource: domain.NamespacedResource{
				Namespace: "kube-system",
			},
		},
		ExternalIP: *apiServerIPAddr,
		CAData:     k8sConfig.CAData,
	}
	k8sConfigUser := domain.Identity{
		Name:     "#kubeconfig-user" + k8sConfig.Username,
		CertData: k8sConfig.CertData,
		KeyData:  k8sConfig.KeyData,
	}
	err = mb.Publish(domain.NewEntities{
		Entities:   []domain.Entity{apiServerPod},
		Identities: []domain.Identity{k8sConfigUser}})
	if err != nil {
		fmt.Printf("Couldn't add apiServer as new entity to bus: %s", err.Error())
	}

	go populateEntities(ctx, channel)
	pods := []domain.Entity{}
	for p := range channel {
		pods = append(pods, p)
	}
	err = mb.Publish(domain.NewEntities{Entities: pods})
	if err != nil {
		fmt.Printf("Couldn't publish newEntity event: %s", err.Error())
	}
}

func populateEntities(ctx context.Context, channel chan<- domain.Entity) {
	defer close(channel)
	client, err := k8s.NewK8sClient("")
	if err != nil {
		panic(err)
	}

	deployments, err := k8s.GetDeployments(ctx, client)
	if err != nil {
		panic(err)
	}
	for _, d := range deployments {
		channel <- d
	}

	pods, err := k8s.GetPods(ctx, client)
	if err != nil {
		panic(err)
	}
	for _, p := range pods {
		channel <- p
	}

}
func ServeFiles(ctx context.Context, port uint) {
	http.Handle("/", http.FileServer(http.Dir("../static")))
	p := strconv.FormatUint(uint64(port), 10)
	http.ListenAndServe(":"+p, nil)
}
