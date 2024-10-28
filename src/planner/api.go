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
		domain.StartListener{Port: 1337},
		domain.StartC2Redirector{DstPort: 1337},
		// domain.KubectlExec{}
	}

	// start listner "Primary" :1337
	// start redirector: ngrok 8080 -> "Primary"
	//

	return plan
}

func StartApi(mb bus.MessageBus) {
	err := mb.Publish(APIStarted{})
	if err != nil {
		panic(err)
	}

	_ = simplePlan()
	// fmt.Print(plan)
}
