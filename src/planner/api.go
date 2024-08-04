package planner

import (
	"fmt"

	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal"
)

type APIStarted struct {
}

func (c APIStarted) EventName() string {
	return "PlannerStarted"
}

func simplePlan() []domain.Command {
	plan := []domain.Command{
		domain.StartListener{Port: 1337},
		domain.StartC2Redirector{DstPort: 1337},
	}
	return plan
}

func StartApi(mb bus.MessageBus) {
	err := mb.Publish(APIStarted{})
	if err != nil {
		panic(err)
	}

	plan := simplePlan()
	fmt.Print(plan)
}
