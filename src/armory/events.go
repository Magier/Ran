package armory

import "github.com/Magier/Ran/domain"

type Loaded struct {
	domain.EventImpl
	TTPs []domain.TTP `json:"ttps"`
}

func (e Loaded) String() string {
	return "loaded"
}
