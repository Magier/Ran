package campaign

import (
	"errors"
	"fmt"
	"log/slog"
	"strings"

	"github.com/Magier/Ran/domain"
)

func (c Campaign) getSystemForExecution(procedure domain.Procedure, target domain.Entity) (domain.Entity, error) {
	// Find best system to execute the TTP on based on a few heuristics:
	// 1) if the TTP targets the pod, and it's compromised, use it
	// 2) if the TTP targets a service account, use the pod and check 1)
	// 3) for now just pick any compromised system (Pod or Node) in the cluster
	if pod, ok := target.(domain.Pod); ok {
		if pod.CanExecuteProcedure(procedure) {
			return pod, nil
		}
	} else if sa, ok := target.(domain.ServiceAccount); ok {
		if owner, ok := c.getServiceAccountOwner(sa); ok {
			if owner.CanExecuteProcedure(procedure) {
				return owner, nil
			}
		}
	}

	c2, ok := c.GetC2("Ran") // ensure the C2 is loaded
	if !ok {
		return nil, fmt.Errorf("C2 system 'Ran' not found in the knowledge base")
	}

	compromisedSystems := make([]domain.Entity, 0)
	for _, entity := range c.kb.GetEntities() {
		if sys, ok := entity.(domain.System); ok {
			if sys.CanExecuteProcedure(procedure) {
				compromisedSystems = append(compromisedSystems, sys)
			}
		}
	}

	// TODO generalize this
	// look for paths from compromised systems to the target
	if node, ok := target.(domain.K8sNode); ok {
		paths, err := c.kb.GetAllPaths(c2.GetId(), node.GetId())

		// entities, relations, err := c.kb.GetPath(c2.GetId(), node.GetId())
		if err != nil {
			return nil, fmt.Errorf("Failed to get path from C2 to target node '%s': %s", node.GetName(), err.Error())
		}

		for _, path := range paths {
			// the last node is the target entity, so check if the one before that is already compromised

			for _, compromisedSys := range compromisedSystems {
				srcBeforeTarget := path.Nodes[len(path.Nodes)-2]
				rel := path.Relations[len(path.Relations)-1]

				// possible system to execution TTP from, check if relations support it
				if compromisedSys.GetId() == srcBeforeTarget.GetId() {
					switch rel.(type) {
					case domain.CanAccess:
						return compromisedSys, nil
					case domain.MountsHostPaths:
						return compromisedSys, nil
					}
				}
			}
		}
	}

	// simply heuristic: continue attacking from the previous foothold
	if c.lastExecSystem != nil {
		slog.Info(fmt.Sprintf("Continuing from last execution system: %s", c.lastExecSystem.GetName()))
		return c.lastExecSystem, nil
	} else if len(compromisedSystems) > 0 {
		// TODO: use heuristic to pick the best system, e.g.
		// - preference to execute on the same system as the last TTPs (or opposite, to make detection more challenging?)
		slog.Warn(fmt.Sprintf("No match for TTP execution found, using first best compromised system: %s", compromisedSystems[0].GetName()))
		return compromisedSystems[0], nil
	}

	return nil, fmt.Errorf("No suitable system found for execution")
}

func findC2Channel(kg KnowledgeBase, finalTarget domain.Entity) (domain.C2Channel, error) {
	if finalTarget == nil {
		return nil, errors.New("Can't find a C2 channel if target is nil")
	}

	var c2Channel domain.C2Channel
	var lastSegment domain.C2Channel
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
					ch := &domain.PodExecC2Channel{
						SourceId: canAccess.SourceId,
						Target:   relTarget,
						Identity: canAccess.Identity,
					}

					// set a pointer to the next channel, the C2 execution component can chain the channels
					if lastSegment != nil {
						c2Channel.SetNextChannel(ch)
					} else {
						c2Channel = ch
					}
					lastSegment = ch
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
	for ch := c2Channel; ch != nil; ch = ch.GetNextChannel() {
		hops = append(hops, ch.GetTargetId())
	}
	slog.Info(fmt.Sprintf("Found C2 channel %s -> %s", c2Channel.GetSourceId(), strings.Join(hops, " -> ")))
	return c2Channel, nil
}
