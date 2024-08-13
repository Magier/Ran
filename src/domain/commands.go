package domain

type Command interface {
	MessageName() string
}

type StartListener struct {
	Port int
}

func (c StartListener) MessageName() string {
	return "StartListener"
}

type StartC2Redirector struct {
	DstPort int
}

func (c StartC2Redirector) MessageName() string {
	return "StartC2Redirector"
}
