package domain

import "fmt"

type ProbBool float32

func (b *ProbBool) Update(delta float32) {
	*b += ProbBool(delta)
	// boundary checks
	if *b < 0 {
		*b = 0
	}
	if *b > 1 {
		*b = 1
	}
}
func (b ProbBool) Bool() bool {
	return b > 0.5
}

func (b ProbBool) String() string {
	if b == 0.5 { // if it's the default value, there is 0 information
		return ""
	}
	return fmt.Sprintf("%t (%.1f%%)", b.Bool(), b*100)
}

func (b ProbBool) IsZero() bool {
	// either unknown, or known to be false
	return b == 0.5 || b == 0
}
func (b ProbBool) IsUnknown() bool {
	return b == 0.5
}

func NewProbBool() ProbBool {
	return 0.5
}

func AsProbBool(val bool) ProbBool {
	if val {
		return 1
	} else {
		return 0
	}
}
