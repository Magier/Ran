package c2

import "fmt"

type ListenerReady struct {
	Name string
	Port int
	// protocol string
}

func (c ListenerReady) MessageName() string {
	return "ListenerReady"
}
func (c ListenerReady) String() string {
	return fmt.Sprintf("Listener '%s:%d' ready", c.Name, c.Port)
}
