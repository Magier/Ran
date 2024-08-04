package domain

type Command interface {
	CommandName() string
}

type StartListener struct {
	Port int
}

func (c StartListener) CommandName() string {
	return "StartListener"
}

type StartC2Redirector struct {
	DstPort int
}

func (c StartC2Redirector) CommandName() string {
	return "StartC2Redirector"
}
