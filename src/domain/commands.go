package domain

import "fmt"

type Command interface {
	Message
	String() string
}

type StartListener struct {
	Port int
}

func (c StartListener) MessageName() string {
	return "StartListener"
}
func (c StartListener) String() string {
	return fmt.Sprintf("Listener on port %d started", c.Port)
}

type StartC2Redirector struct {
	DstPort int
}

func (c StartC2Redirector) MessageName() string {
	return "StartC2Redirector"
}
func (c StartC2Redirector) String() string {
	return "Started C2 redirector"
}
