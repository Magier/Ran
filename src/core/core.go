package core

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"sync"
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
	Bus      bus.MessageBus
	Armory   *armory.Armory
	Campaign *campaign.Campaign
	C2       c2.C2Manager
	target   string
	ctx      context.Context
}

type Config struct {
	SliverConfigPath string
	PlanPath         string
	ArmoryDir        string
}

func InitRan(target, armoryDir string) Ran {
	if armoryDir == "" {
		armoryDir = "./armory/"
	}

	path, _ := os.Getwd()
	// Temporarily fix for running from the root of the project
	if filepath.Base(path) == "RanUI" {
		path = filepath.Dir(path)
	}

	mb := bus.CreateMessageBus()
	a := &armory.Armory{SrcDir: filepath.Join(path, armoryDir)}
	c := campaign.StartCampaign(mb, a)
	c2 := c2.InitC2Manager(mb)

	ran := Ran{
		Bus:      mb,
		Armory:   a,
		Campaign: c,
		C2:       c2,
		target:   target,
	}

	return ran
}

func (r *Ran) ExecuteAtomicTTP(ctx context.Context, ttpID, target string) error {
	if ttpID == "" {
		return errors.New("No TTP ID provided")
	}
	err := r.Start(ctx, true, "")
	if err != nil {
		return fmt.Errorf("Couldn't start Ran: %v", err)
	}

	var ttp domain.TTP
	ttp, ok := r.Armory.GetTTP(ttpID)
	if !ok {
		panic(fmt.Sprintf("Couldn't get TTP '%s'", ttpID))
	}

	args := make(map[string]string)

	err = r.SetTarget(target)
	if err != nil {
		return fmt.Errorf("Couldn't set target: %v", err.Error())
	}

	msg, err := r.Campaign.GroundAction(ttp, target, "", args)
	if err != nil {
		panic(fmt.Sprintf("Couldn't ground action: %s", err.Error()))
	}

	var wg sync.WaitGroup

	r.Bus.Subscribe(domain.TTPExecuted{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		defer wg.Done()

		if e, ok := msg.(domain.TTPExecuted); ok {
			if e.Success {
				fmt.Printf("✅ TTP '%s' executed successfully on %s \n", ttpID, target)
				return nil, nil
			}
			if len(e.Results) > 0 {
				fmt.Printf("❌ TTP '%s' failed to execute: %s\n", ttpID, e.Results[0])
			} else {
				fmt.Printf("❌ TTP '%s' failed to execute (no results)\n", ttpID)
			}
		}
		return nil, nil
	})

	wg.Add(1)
	err = r.Bus.Publish(msg)
	if err != nil {
		panic(fmt.Sprintf("Couldn't publish action: %s", err.Error()))
	}
	// TODO: execute cleanup
	wg.Wait()
	return nil
}

func (r *Ran) Start(ctx context.Context, loadKubeConfig bool, planPath string) error {
	r.ctx = ctx
	err := r.Armory.Load()
	if err != nil {
		return err
	} else {
		err = r.Bus.Publish(armory.Loaded{
			TTPs: r.Armory.GetTTPs(),
		})
		if err != nil {
			return fmt.Errorf("Couldn't publish Armory.Loaded event: %s", err.Error())
		}
	}

	// TODO: turn fileshare into a regular action
	// filesharePort, _ := r.campaign.GetFileshare()
	// go ServeFiles(ctx, filesharePort)
	go func() {
		err := r.C2.Start(ctx)
		if err != nil {
			slog.Error(fmt.Sprintf("Couldn't start C2: %s", err.Error()))
		}
	}()
	// planner.StartAPI(r.Bus)
	go r.Bus.HandleEvents(ctx)

	// load initial entities from the target cluster into the campaign
	err = r.InitCampaign(ctx, loadKubeConfig)
	if err != nil {
		return fmt.Errorf("Couldn't initialize campaign: %s", err.Error())
	}
	r.Bus.Subscribe(domain.CampaignReset{}, func(ctx context.Context, msg domain.Message) (domain.Message, error) {
		err := r.InitCampaign(ctx, loadKubeConfig)
		return nil, err
	})

	// ensure target is set after the cluster, so it's properly associated with the cluster
	if r.target != "" {
		err = r.SetTarget(r.target)
		if err != nil {
			return fmt.Errorf("Couldn't set target: %s", err.Error())
		}
	}

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
	return nil
}

func (r Ran) InitCampaign(ctx context.Context, loadKubeConfig bool) error {
	namespaces := []string{}
	initialFacts := make(chan MaybeNewFacts, 1)
	go loadInitialEntities(ctx, initialFacts, loadKubeConfig, namespaces)
	for update := range initialFacts {
		if update.Error != nil {
			_ = r.Bus.Publish(domain.ErrorMsg{
				Level: domain.LevelFatal,
				Msg:   update.Error.Error(),
			})
		} else {
			ev, err := r.Campaign.UpdateFacts(update.NewFacts, campaign.RemovedFacts{})
			if err != nil {
				return fmt.Errorf("Couldn't update facts: %s", err.Error())
			} else {
				if err = r.Bus.Publish(ev); err != nil {
					return fmt.Errorf("Couldn't publish facts changed event: %s", err.Error())
				}
			}
		}
	}
	return nil
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
	ns, kind, name, err := campaign.UnpackResourceID(target)
	if err != nil {
		return err
	}
	target = name

	client, err := k8s.NewK8sClient("")
	if err != nil {
		return fmt.Errorf("could not create K8s client: %v", err.Error())
	}

	if kind == "pod" {
		_, err = client.GetPod(r.ctx, ns, target)
		if err != nil {
			return fmt.Errorf("no pod found: %v", err.Error())
		}
	}

	msg, err := r.Campaign.SetTarget(ns, target)
	if err != nil {
		return err
	}
	err = r.Bus.Publish(msg)
	return err
}

type MaybeEntity struct {
	Entity domain.Entity
	Error  error
}

type MaybeNewFacts struct {
	NewFacts campaign.NewFacts
	Error    error
}

func loadInitialEntities(ctx context.Context, results chan<- MaybeNewFacts, loadAll bool, namespaces []string) {
	defer close(results)

	client, err := k8s.NewK8sClient("")
	if err != nil {
		results <- MaybeNewFacts{Error: fmt.Errorf("Couldn't create K8s client for context %s (%v)", client.Context.Name, err)}
		return
	} else if !client.TestConnection() {
		results <- MaybeNewFacts{Error: fmt.Errorf("Can't connect to %s (%v)", client.Context.Name, err)}
		return
	}

	cluster := domain.NewCluster(client.Context.Name, client.Config.Host)
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

		k8sConfigUser := domain.User{
			Name:     client.Context.Name,
			IsAdmin:  true,
			CertData: client.Context.UserCert,
			KeyData:  client.Context.UserKey,
			Entitlements: []domain.RBACPermission{{
				Verb:         "*",
				ResourceType: "*",
				Scope:        "*",
			}},
		}

		identities = append(identities, k8sConfigUser)
	}
	slog.Info("Sending inital entities like cluster")
	results <- MaybeNewFacts{NewFacts: campaign.NewFacts{
		Entities:   entities,
		Identities: identities,
		Relations:  relations,
	}}

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
			results <- MaybeNewFacts{NewFacts: campaign.NewFacts{Entities: entities}}
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
