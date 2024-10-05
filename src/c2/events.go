package c2

import (
	"fmt"
	"net"

	"github.com/Magier/Ran/domain"
)

type ListenerReady struct {
	Name     string
	IP       net.IP
	Port     uint
	Protocol domain.Protocol
}

func (c ListenerReady) MessageName() string {
	return "ListenerReady"
}
func (c ListenerReady) String() string {
	return fmt.Sprintf("Listener '%s:%d' ready", c.Name, c.Port)
}
