package core

import (
	"context"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
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
)

type Ran struct {
	Bus              bus.MessageBus
	Armory           *armory.Armory
	Campaign         *campaign.Campaign
	sliverConfigPath string
	target           string
	ctx              context.Context
	// c2       *c2.C2
}

type Config struct {
	SliverConfigPath string
	PlanPath         string
	ArmoryDir        string
}

func InitRan(target, armoryDir, sliverConfigPath string) Ran {
	if armoryDir == "" {
		armoryDir = "./armory/"
	}

	if sliverConfigPath == "" {
		sliverConfigPath = "sliver_cfg.json"
	}

	path, _ := os.Getwd()
	// Temporarily fix for running from the root of the project
	if filepath.Base(path) == "RanUI" {
		path = filepath.Dir(path)
	}

	mb := bus.CreateMessageBus()
	a := &armory.Armory{SrcDir: filepath.Join(path, armoryDir)}
	c := campaign.StartCampaign(mb, a)

	ran := Ran{
		Bus:              mb,
		Armory:           a,
		Campaign:         c,
		sliverConfigPath: filepath.Join(path, sliverConfigPath),
		target:           target,
	}

	return ran
}

func (r *Ran) Start(ctx context.Context, loadKubeConfig bool, planPath string) {
	r.ctx = ctx
	err := r.Armory.Load()
	if err != nil {
		panic(fmt.Sprintf("Couldn't load armory: %s", err.Error()))
	} else {
		err = r.Bus.Publish(armory.Loaded{
			TTPs: r.Armory.GetTTPs(),
		})
		if err != nil {
			panic(fmt.Sprintf("Couldn't publish Armory.Loaded event: %s", err.Error()))
		}
	}

	// TODO: turn fileshare into a regular action
	// filesharePort, _ := r.campaign.GetFileshare()
	// go ServeFiles(ctx, filesharePort)
	go c2.StartC2(ctx, r.Bus, r.sliverConfigPath)
	// planner.StartAPI(r.Bus)

	go r.Bus.HandleEvents(ctx)
	// TODO maybe switch between TUI and web-UI (start frontend as well?)

	namespaces := []string{}
	go func() {
		loadInitialEntities(ctx, r.Bus, loadKubeConfig, namespaces)
		// ensure target is set after the cluster, so it's properly associated with the cluster
		if r.target != "" {
			r.SetTarget(r.target)
		}
	}()

	if planPath != "" {
		p := planner.CreatePlanner(planPath, r.Armory, r.Bus)
		go p.Execute(ctx)
	}

	go func() {
		time.Sleep(200 * time.Millisecond)
		if err := r.Bus.Publish(domain.RanReady{}); err != nil {
			panic(err.Error())
		}
	}()
}

func (r Ran) ReplayEvents() {
	// TODO: replay the events that were actually already sent to the bus, instead of creating new events
	err := r.Bus.Publish(armory.Loaded{
		TTPs: r.Armory.GetTTPs(),
	})
	if err != nil {
		panic(fmt.Sprintf("Couldn't publish ArmoryLoaded event: %s", err.Error()))
	}
}

func (r *Ran) Subscribe(event domain.Event, handler domain.MessageHandler) {
	r.Bus.Subscribe(event, handler)
}

func (r *Ran) SubscribeToName(name string, handler domain.MessageHandler) {
	r.Bus.SubscribeToName(name, handler)
}

func (r *Ran) SetTarget(target string) error {
	// check if target is a valid entity
	ns := "default"
	if strings.Contains(target, "/") {
		parts := strings.Split(target, "/")
		if len(parts) == 2 {
			ns = parts[0]
			target = parts[1]
		} else if parts[0] == "ns" && len(parts) == 4 {
			// it's the ID format `ns/<ns>/<kind>/<podname>`
			ns = parts[1]
			target = parts[3]
		} else {
			return fmt.Errorf("invalid target format")
		}
	}

	client, err := k8s.NewK8sClient("")
	if err != nil {
		return fmt.Errorf("could not create K8s client: %v", err.Error())
	}
	_, err = client.GetPod(r.ctx, ns, target)
	if err != nil {
		return fmt.Errorf("no pod found: %v", err.Error())
	}

	facts := campaign.SetTarget(fmt.Sprintf("%s/%s", ns, target))
	err = r.Bus.Publish(facts)
	if err != nil {
		panic(fmt.Sprintf("Couldn't set target: %s", err.Error()))
	}
	return nil
}

type MaybeEntity struct {
	Entity domain.Entity
	Error  error
}

func loadInitialEntities(ctx context.Context, mb bus.MessageBus, loadAll bool, namespaces []string) {
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

func ServeFiles(ctx context.Context, port uint) error {
	http.Handle("/", http.FileServer(http.Dir("../static")))
	p := strconv.FormatUint(uint64(port), 10)
	return http.ListenAndServe(":"+p, nil)
}
