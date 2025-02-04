package core

import (
	"context"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"strconv"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/domain"
	"github.com/Magier/Ran/internal/bus"
	k8s "github.com/Magier/Ran/k8sclient"
	"github.com/Magier/Ran/planner"
	"github.com/Magier/Ran/tui"
	tea "github.com/charmbracelet/bubbletea"
)

func StartRan(withTui bool, loadKubeConfig bool) {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)

	// ctx, cancel := context.WithCancel(context.Background(), os.Interrupt)
	defer cancel()
	mb := bus.CreateMessageBus()
	a, err := armory.LoadArmory("../armory/")
	if err != nil {
		panic(err)
	}
	c := campaign.StartCampaign(mb, a)
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
		namespaces := []string{
			"default",
			"restricted-ns",
		}
		go loadClusterData(ctx, mb, namespaces)
	}

	if ui != nil {
		tui.RunTUI(ui)
	}
}

type MaybeEntity struct {
	Entity domain.Entity
	Error  error
}

func loadClusterData(ctx context.Context, mb bus.MessageBus, namespaces []string) {
	client, err := k8s.NewK8sClient("")
	if err != nil {
		_ = mb.Publish(domain.ErrorMsg{
			Level: domain.LevelFatal,
			Msg:   "Couldn't create K8s client for context " + client.Context.Name,
		})
		return
	} else if !client.TestConnection() {
		_ = mb.Publish(domain.ErrorMsg{
			Level: domain.LevelFatal,
			Msg:   "Can't connect to " + client.Context.Name,
		})
		return
	}

	addApiServer := false
	entities := make([]domain.Entity, 0)
	if addApiServer {
		apiServerPod, err := client.GetApiServer()
		if err != nil {
			slog.Error(fmt.Sprintf("Couldn't resolve apiServer IP: %s", err.Error()))
		} else {
			entities = append(entities, apiServerPod)
		}
	}

	k8sConfigUser := domain.Identity{
		Name:     client.Context.Name,
		Kind:     domain.AdminUser,
		CertData: client.Context.UserCert,
		KeyData:  client.Context.UserKey,
		Permissions: []domain.RbacPermission{{
			Verbs:         []string{"*"},
			ResourceTypes: []string{"*"},
			Scope:         "*",
		}},
	}
	err = mb.Publish(domain.FactsChanged{
		NewEntities:   entities,
		NewIdentities: []domain.Identity{k8sConfigUser}})
	if err != nil {
		fmt.Printf("Couldn't add apiServer as new entity to bus: %s", err.Error())
	}

	channel := make(chan MaybeEntity)
	go populateEntities(ctx, namespaces, channel)

	entities = []domain.Entity{}
	for maybeEntity := range channel {
		if maybeEntity.Error != nil {
			slog.Error(maybeEntity.Error.Error())
		} else {
			entities = append(entities, maybeEntity.Entity)
		}
	}
	if len(entities) > 0 {
		err = mb.Publish(domain.FactsChanged{NewEntities: entities})
		if err != nil {
			fmt.Printf("Couldn't publish newEntity event: %s", err.Error())
		}
	} else {
		slog.Warn("No pods found at for initialization!")
	}
}

func populateEntities(ctx context.Context, namespaces []string, channel chan<- MaybeEntity) {
	defer close(channel)
	client, err := k8s.NewK8sClient("")
	if err != nil {
		channel <- MaybeEntity{Entity: nil, Error: err}
		return
	}

	// no restriction of namespaces -> load all namespaces indicated to K8s client by namespace ""
	if len(namespaces) == 0 {
		namespaces = []string{""}
	}

	cluster := domain.Cluster{Name: client.Context.Name, Address: client.Config.Host}
	channel <- MaybeEntity{Entity: cluster, Error: err}

	for _, nsName := range namespaces {
		deployments, err := client.GetDeployments(ctx, nsName)
		if err != nil {
			channel <- MaybeEntity{Entity: nil, Error: err}
			return
		} else {
			for _, d := range deployments {
				channel <- MaybeEntity{Entity: d, Error: nil}
			}
		}

		pods, err := client.GetPods(ctx, nsName)
		if err != nil {
			channel <- MaybeEntity{Entity: nil, Error: err}
		} else {
			for _, p := range pods {
				channel <- MaybeEntity{Entity: p, Error: nil}
			}
		}
	}
}

func ServeFiles(ctx context.Context, port uint) {
	http.Handle("/", http.FileServer(http.Dir("../static")))
	p := strconv.FormatUint(uint64(port), 10)
	http.ListenAndServe(":"+p, nil)
}
