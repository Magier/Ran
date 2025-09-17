package c2

import (
	"fmt"
	"net"

	"github.com/Magier/Ran/domain"
)

type ListenerReady struct {
	domain.EventImpl
	ID       string
	Name     string
	C2Name   string
	IP       net.IP
	Port     uint
	Protocol domain.Protocol
}

func (c ListenerReady) String() string {
	return fmt.Sprintf("Listener '%s:%d' ready", c.Name, c.Port)
}

type ListenerStopped struct {
	domain.EventImpl
	ListenerID string
	C2Name     string
	Name       string
	Port       uint
}

func (c ListenerStopped) String() string {
	return fmt.Sprintf("Listener '%s' stopped", c.Name)
}

type SessionStarted struct {
	domain.EventImpl
	C2Kind     string
	C2Endpoint string
	Session    domain.Session
}

func (c SessionStarted) String() string {
	return "Session started: " + c.Session.Id
}

type SessionClosed struct {
	domain.EventImpl
	Session domain.Session
}

func (c SessionClosed) String() string {
	return "Session closed: " + c.Session.Id
}

type BinaryDownloaded struct {
	Name string
	Path string
}

type ImplantGenerated struct {
	domain.EventImpl
	C2Name string
	Name   string
	Path   string
}

func (e ImplantGenerated) String() string {
	return fmt.Sprintf("Implant '%s' generated %s", e.Name, e.Path)
}
