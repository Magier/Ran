package planner

import (
	"os"

	attackflow "github.com/Magier/Ran/attack_flow"
	bus "github.com/Magier/Ran/internal/bus"
)

type Planner interface {
	Execute()
}

func CreatePlanner(path string, mb bus.MessageBus) Planner {
	return PlayBookPlanner{}
}

type PlayBookPlanner struct {
	Steps []string
	bus   bus.MessageBus
}

func (p *PlayBookPlanner) LoadPlan(path string) {
	data, err := os.ReadFile(path)
	if err == nil {
		af, err := attackflow.UnmarshalAttackFlow(data)
		if err != nil {
			println(err.Error())
		}
		var _ = af
	}
}

func (p PlayBookPlanner) Execute() {
	// TODO execute step after step
}
