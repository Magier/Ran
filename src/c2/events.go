package c2

import (
	"fmt"
	"net"

	"github.com/Magier/Ran/domain"
)

type ListenerReady struct {
	Id       string
	Name     string
	IP       net.IP
	Port     uint
	Protocol domain.Protocol
}

func (c ListenerReady) String() string {
	return fmt.Sprintf("Listener '%s:%d' ready", c.Name, c.Port)
}

type ListenerStopped struct {
	Name string
	Port uint
}

func (c ListenerStopped) String() string {
	return fmt.Sprintf("Listener '%s' stopped", c.Name)
}
