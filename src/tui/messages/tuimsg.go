package tuimsg

import "github.com/Magier/Ran/domain"

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

type State map[string]int
type StateChanged struct {
	State State
}

func NewState() State {
	return make(State)
}

func (s State) Update(key string, numChange int) State {
	prevNum, exists := s[key]
	if !exists {
		prevNum = 0
	}

	s[key] = prevNum + numChange
	return s
}
