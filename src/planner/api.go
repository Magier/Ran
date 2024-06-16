package planner

import bus "github.com/Magier/Ran/internal"

type APIStarted struct {
}

func (c APIStarted) EventName() string {
	return "planner"
}

func StartApi(mb bus.MessageBus) {
	err := mb.Publish(APIStarted{})
	if err != nil {
		panic(err)
	}
}
