package campaign

import (
	"errors"
	"fmt"
	"log/slog"
	"strings"

	"github.com/Magier/Ran/domain"
)

func findC2Channel(kg KnowledgeBase, finalTarget domain.Entity) (domain.C2Channel, error) {
	if finalTarget == nil {
		return nil, errors.New("Can't find a C2 channel if target is nil")
	}

	var c2Channel domain.PodExecC2Channel
	var lastSegment *domain.PodExecC2Channel
	for _, c2 := range kg.GetC2s() {
		paths, err := kg.GetPath(c2.GetId(), finalTarget.GetId())
		if err != nil {
			if !strings.HasPrefix(err.Error(), "target vertex not reachable") {
				slog.Debug(fmt.Sprintf("Failed to get path from '%s' to '%s'", c2.GetId(), finalTarget.GetId()))
			}
			continue
		}

		for _, rel := range paths.Relations {
			if ch, ok := rel.(domain.C2Channel); ok {
				return ch, nil
			} else if canAccess, ok := rel.(domain.CanAccess); ok {
				if relTarget, ok := kg.GetEntity(rel.GetTargetId()); ok {
					ch := domain.PodExecC2Channel{
						SourceId: canAccess.SourceId,
						Target:   relTarget,
						Identity: canAccess.Identity,
					}

					// set a pointer to the next channel, the C2 execution component can chain the channels
					if lastSegment != nil {
						c2Channel.NextChannel = &ch
					} else {
						c2Channel = ch
					}
					lastSegment = &ch
				} else {
					return nil, fmt.Errorf("Could not identify target %s", canAccess.TargetId)
				}

			}
		}
	}

	if lastSegment == nil {
		return c2Channel, fmt.Errorf("No channel found")
	}
	hops := []string{}
	for ch := &c2Channel; ch != nil; ch = ch.NextChannel {
		hops = append(hops, ch.Target.GetId())
	}
	slog.Info(fmt.Sprintf("Found C2 channel %s -> %s", c2Channel.SourceId, strings.Join(hops, " -> ")))
	return c2Channel, nil
}
