package planner

import (
	"github.com/Magier/Ran/domain"
	bus "github.com/Magier/Ran/internal/bus"
)

type APIStarted struct {
}

func (c APIStarted) String() string {
	return "API started"
}

func simplePlan() []domain.Command {
	plan := []domain.Command{
		domain.StartListener{CommandImpl: domain.NewCmd(), Port: 1337},
		domain.StartC2Redirector{CommandImpl: domain.NewCmd(), DstPort: 1337},
		// domain.KubectlExec{}
	}

	// start listner "Primary" :1337
	// start redirector: ngrok 8080 -> "Primary"
	//

	return plan
}

func StartAPI(mb bus.MessageBus) {
	err := mb.Publish(APIStarted{})
	if err != nil {
		panic(err)
	}

	_ = simplePlan()
	// fmt.Print(plan)
}
