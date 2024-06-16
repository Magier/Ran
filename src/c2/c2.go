package c2

import bus "github.com/Magier/Ran/internal"

type C2Started struct {
}

func (c C2Started) EventName() string {
	return "campaign"
}

func StartC2(mb bus.MessageBus) {
	startListener()
	err := mb.Publish(C2Started{})
	if err != nil {
		panic(err)
	}
}

func startListener() {

}
