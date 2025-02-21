package domain

import "fmt"

type ProbBool float32

func (b ProbBool) Bool() bool {
	return b > 0.5
}

func (b ProbBool) String() string {
	if b == 0.5 { // if it's the default value, there is 0 information
		return ""
	}
	return fmt.Sprintf("%t (%.1f%%)", b.Bool(), b*100)
}

func NewProbBool(val bool) ProbBool {
	if val {
		return 1
	} else {
		return 0
	}
}
