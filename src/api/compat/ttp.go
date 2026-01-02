package compat

import (
	"github.com/Magier/Ran/api"
	"github.com/Magier/Ran/domain"
)

var _ domain.TTP = domain.TTP(api.TTP{})
