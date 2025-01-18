package c2

import (
	"strings"

	"github.com/Magier/Ran/domain"
)

func handleExecTTPResult(exec domain.ExecTTP, stdout, stderr string) (domain.Message, error) {
	if stdout == "" {
		if strings.Contains(stderr, ": not found") {
			return domain.TTPFailed{
				Reason: stderr,
				TTP:    exec.TTP,
			}, nil
		}
	}

	return nil, nil
}
