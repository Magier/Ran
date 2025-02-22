package tuimsg

import (
	"github.com/Magier/Ran/domain"
)

type EntitySelected struct {
	Id          string
	Kind        string
	Name        string
	AccessLevel domain.AccessLevel
}

// ensure the message is compatible with the abstract Entity
var _ domain.Entity = (*EntitySelected)(nil)

func (e EntitySelected) GetId() string {
	return e.Id
}

func (e EntitySelected) GetKind() string {
	return e.Kind
}

func (e EntitySelected) GetName() string {
	return e.Name
}

type StateChanged struct {
	State domain.State
}

func NewState() domain.State {
	return make(domain.State)
}

type ContentFilterStarted struct {
}

type ContentFilterStopped struct {
}
