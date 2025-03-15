package core

import (
	"context"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"strings"
	"time"

	"github.com/Magier/Ran/armory"
	"github.com/Magier/Ran/c2"
	"github.com/Magier/Ran/campaign"
	"github.com/Magier/Ran/core/bus"
	"github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
	"github.com/Magier/Ran/planner"
	"github.com/Magier/Ran/tui"
	tea "github.com/charmbracelet/bubbletea"
)

type Ran struct {
	Bus      bus.MessageBus
	armory   armory.Armory
	campaign *campaign.Campaign
	// c2       *c2.C2
}

func InitRan() Ran {
	mb := bus.CreateMessageBus()
	a := armory.Armory{}
	c := campaign.StartCampaign(mb, a)
	ran := Ran{
		Bus:      mb,
		armory:   a,
		campaign: c,
	}
	return ran
}

func (r *Ran) Start(withTui bool, loadKubeConfig bool, target string, planPath string) {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	// ctx, cancel := context.WithCancel(context.Background(), os.Interrupt)
	defer cancel()
	var ui *tea.Program = nil
	if withTui {
		ui = tui.SetupTUI(r.Bus, r.campaign, r.armory)
	}

	err := r.armory.Load("../armory/")
	if err != nil {
		panic(fmt.Sprintf("Couldn't load armory: %s", err.Error()))
	} else {
		err = r.Bus.Publish(armory.Loaded{
			TTPs: r.armory.GetTTPs(),
		})
		if err != nil {
			panic(fmt.Sprintf("Couldn't publish ArmoryLoaded event: %s", err.Error()))
		}
	}

	// TODO: turn fileshare into a regular action
	filesharePort, _ := r.campaign.GetFileshare()
	go ServeFiles(ctx, filesharePort)
	go c2.StartC2(ctx, r.Bus)
	// planner.StartAPI(r.Bus)

	go r.Bus.HandleEvents(ctx)
	// TODO maybe switch between TUI and web-UI (start frontend as well?)

	namespaces := []string{}
	go loadInitialEntities(ctx, r.Bus, loadKubeConfig, target, namespaces)

	if planPath != "" {
		p := planner.CreatePlanner(planPath, r.armory, r.Bus)
		go p.Execute(ctx)
	}

	go func() {
		time.Sleep(200 * time.Millisecond)
		if err := r.Bus.Publish(domain.RanReady{}); err != nil {
			panic(err.Error())
		}
	}()

	if ui != nil {
		tui.RunTUI(ui)
	}
}

type MaybeEntity struct {
	Entity domain.Entity
	Error  error
}

func loadInitialEntities(ctx context.Context, mb bus.MessageBus, loadAll bool, target string, namespaces []string) {
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
	cluster := domain.Cluster{Name: client.Context.Name, Address: client.Config.Host}
	entities := []domain.Entity{cluster}
	identities := []domain.Identity{}
	relations := []domain.Relation{}

	if loadAll {
		addApiServer := false
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

		identities = append(identities, k8sConfigUser)
	} else if target != "" {
		ns := "default"
		if strings.Contains(target, "/") {
			parts := strings.SplitN(target, "/", 2)
			ns = parts[0]
			target = parts[1]
		}

		initialPod := domain.NewPod(target, ns)

		initialAccessRelation := domain.CanAccess{
			SourceId: "c2/Ran",
			TargetId: initialPod.GetId(),
			// Identity:    identity,
			AccessLevel: domain.UserExec,
		}
		initialPod.AccessLevel = domain.UserExec
		entities = append(entities, initialPod)
		relations = append(relations, initialAccessRelation)
	}

	err = mb.Publish(domain.FactsChanged{
		NewEntities:   entities,
		NewIdentities: identities,
		NewRelations:  relations,
	})
	if err != nil {
		fmt.Printf("Couldn't add initial entities to bus: %s", err.Error())
	}

	// gradually load all entities
	if loadAll {
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
