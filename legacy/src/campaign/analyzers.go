package campaign

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"reflect"
	"regexp"
	"strconv"
	"strings"

	"github.com/Magier/Ran/domain"
	"github.com/google/uuid"
)

func (c Campaign) AnalyzeChanges(new domain.Facts, removed domain.Facts, execSystem domain.System) (domain.Facts, domain.Facts, error) {
	entities := make(map[string]domain.Entity)
	relations := make([]domain.Relation, 0)
	relations = append(relations, new.Relations...)
	identities := make(map[string]domain.Identity)
	assets := make([]domain.Asset, 0)
	queue := append([]domain.Entity{}, new.Entities...)

	var resultingFacts domain.Facts
	var err error

	// Index-based loop to avoid issues with appending while ranging
	for i := 0; i < len(queue); i++ {
		current := queue[i]

		// ensure to always work with the latest and most complete information
		if existing, exists := entities[current.GetId()]; exists {
			current = domain.UpdateEntity(current, existing)
		}

		sys, isSystem := current.(domain.System)
		var isUnknown bool

		switch e := current.(type) {
		case domain.Pod:
			resultingFacts, err = c.analyzePod(e)
		case domain.ServiceAccountToken:
			resultingFacts, err = analyzeServiceAccountToken(e)
			if err == nil {
				// the resulting token in the Assets contains the resolved entities from the raw token
				if len(resultingFacts.Assets) > 0 {
					e = resultingFacts.Assets[0].(domain.ServiceAccountToken)
				}

				if mergedPod, stalePod, ok := c.reconcileTokenPodWithSource(e, execSystem, resultingFacts.Entities); ok {
					resultingFacts.Entities = removeEntityByID(resultingFacts.Entities, stalePod.GetId())
					resultingFacts.Entities = append(resultingFacts.Entities, mergedPod)

					if _, exists := c.GetEntityById(stalePod.GetId()); exists {
						newFacts, removedEdges := c.transplantEdges(stalePod, mergedPod)
						resultingFacts.Update(newFacts)
						removed.Update(removedEdges)
						removed.Entities = append(removed.Entities, stalePod)
					}
				}
			}
		case domain.SelfSubjectRulesReview:
			resultingFacts, _, err = c.analyzeSelfSubjectRulesReview(e)
		case domain.RoleBinding:
			resultingFacts, _, err = c.analyzeRoleBinding(e)
		case domain.ConfigMap:
			resultingFacts, _, err = c.analyzeConfigMap(e)
		case domain.Deployment:
			resultingFacts, err = c.analyzeWorkloadOwnership(e)
		case domain.StatefulSet:
			resultingFacts, err = c.analyzeWorkloadOwnership(e)
		case domain.DaemonSet:
			resultingFacts, err = c.analyzeWorkloadOwnership(e)
		case domain.ReplicaSet:
			resultingFacts, err = c.analyzeWorkloadOwnership(e)
		case domain.CronJob:
			resultingFacts, err = c.analyzeWorkloadOwnership(e)
			entities[e.GetId()] = e
		case domain.UnknownSystem:
			isUnknown = true
			resultingFacts, _, err = c.analyzeUnknownSystem(e)

			// TODO: handle possible promotion of unknown system to known system properly and consistently ... see similar implementation further below
			if len(resultingFacts.Entities) > 0 {
				slog.Info("Analyzed Unknown system resulted in more than 1 entity - handling possible promotion only for the first one.")
			}
			for _, resultingSys := range resultingFacts.Entities {
				// Unknon system was promoted, if the type changed ... assumes only 1 entity is returned

				if _, ok := resultingSys.(domain.UnknownSystem); !ok {
					// update edges in the KB
					new, removedEdges := c.transplantEdges(e, resultingSys)
					resultingFacts.Update(new)
					removed.Update(removedEdges)
					removed.Entities = append(removed.Entities, e)

					for i, r := range relations {
						if r.GetSourceId() == e.GetId() {
							if relocator, ok := r.(domain.RelationRelocator); ok {
								newRel := relocator.WithSource(resultingSys)
								relations[i] = newRel
							}
						} else if r.GetTargetId() == e.GetId() {
							if relocator, ok := r.(domain.RelationRelocator); ok {
								newRel := relocator.WithTarget(resultingSys)
								relations[i] = newRel
							} else {
								slog.Warn("Could not relocate incoming relation", "relation", r.GetRelationName())
							}
						}
					}
				}
			}
		case domain.CloudEnvironment:
			if cluster, ok := c.GetK8sCluster(); ok {
				entities[e.GetId()] = e
				resultingFacts = domain.Facts{
					Relations: []domain.Relation{
						domain.Contains{Container: e, Object: cluster},
					},
				}
			}
		case domain.MetadataServer:
			entities[e.GetId()] = e
			if cluster, ok := c.GetK8sCluster(); ok {
				relations = append(relations, domain.Contains{
					Container: cluster,
					Object:    e,
				})
			} else {
				slog.Error("Metadata Server found, but no Kubernetes cluster in knowledge base")
			}
		case domain.GCPServiceAccount:
			entities[e.GetId()] = e
			if csp, ok := c.GetCloudServiceProvider(); ok {
				resultingFacts = domain.Facts{
					Relations: []domain.Relation{
						domain.Contains{Container: csp, Object: e},
					},
				}
			}
		case domain.GCPBucket:
			entities[e.GetId()] = e
			if csp, ok := c.GetCloudServiceProvider(); ok {
				resultingFacts = domain.Facts{
					Relations: []domain.Relation{
						domain.Contains{Container: csp, Object: e},
					},
				}
			}
		default:
			// just add the entity to the KB and skip ahead to the next entity without analyzing it
			entities[e.GetId()] = e
			slog.Warn("Unknown entity type in analyzer processQueue", "entity", e)
			continue
		}

		if err != nil {
			slog.Error(fmt.Sprintf("Failed to analyze %T", current), "error", err)
		} else {
			if pod, ok := sys.(domain.Pod); ok && !isUnknown {
				knownSystems := c.getSystems(true, false)
				for _, knownSystem := range knownSystems {
					knownPod, isPod := knownSystem.(domain.Pod)
					if !isPod || knownPod.GetId() == pod.GetId() {
						continue
					}

					if !isSameSystem(pod, knownPod) && !podsShareIP(pod, knownPod) {
						continue
					}

					preferred, stale := choosePreferredPodIdentity(pod, knownPod)
					if stale.GetId() == pod.GetId() {
						resultingFacts.Entities = removeEntityByID(resultingFacts.Entities, stale.GetId())
					}
					identifiedPod := domain.UpdateEntity(preferred, stale).(domain.Pod)
					resultingFacts.Entities = append(resultingFacts.Entities, identifiedPod)

					newFacts, removedEdges := c.transplantEdges(stale, identifiedPod)
					resultingFacts.Update(newFacts)
					removed.Update(removedEdges)
					removed.Entities = append(removed.Entities, stale)
					break
				}
			}

			// if the system is known, see if it can be merged with an yet unknown systems
			if isSystem && !isUnknown {
				unknownSystems := c.getSystems(false, true)
				for _, unknownSys := range unknownSystems {
					if isSameSystem(sys, unknownSys) {
						identifiedSystem := domain.UpdateEntity(sys, unknownSys)
						resultingFacts.Entities = append(resultingFacts.Entities, identifiedSystem)
						new, removedEdges := c.transplantEdges(unknownSys, identifiedSystem)
						resultingFacts.Update(new)
						removed.Update(removedEdges)
						removed.Entities = append(removed.Entities, unknownSys)
						break
					}
				}
			}

			// queue newly found entities for analysis, but only if they bring new information
			for _, entity := range resultingFacts.Entities {
				if entity.GetId() == current.GetId() {
					continue
				}
				if existing, alreadySeen := entities[entity.GetId()]; alreadySeen && reflect.DeepEqual(entity, existing) {
					continue
				}
				queue = append(queue, entity)
			}
			mergeAnalyzedFacts(entities, identities, &relations, &assets, resultingFacts)
		}
	}

	for _, r := range new.Relations {
		switch rel := r.(type) {
		case domain.CanAccess:
			target, ok := entities[rel.TargetId]
			if !ok {
				target, ok = c.GetEntityById(rel.TargetId)
				if !ok {
					slog.Warn("Could not find target entity for CanAccess relation", "relation", rel)
					continue
				}
			}
			if sys, ok := target.(domain.System); ok {
				sys.SetAccessLevel(rel.AccessLevel)
				entities[sys.GetId()] = sys
			}
			// Handle CanAccess relation
		default:
		}
	}

	entitiesSlice := make([]domain.Entity, 0, len(entities))
	for _, entity := range entities {
		entitiesSlice = append(entitiesSlice, entity)
	}

	identitySlice := make([]domain.Identity, 0, len(entities))
	for _, i := range identities {
		identitySlice = append(identitySlice, i)
	}

	assetsSlice := make([]domain.Asset, 0, len(assets))
	assetsSlice = append(assetsSlice, assets...)

	return domain.Facts{
		Entities:   entitiesSlice,
		Relations:  relations,
		Assets:     assetsSlice,
		Identities: identitySlice,
	}, removed, nil
}

func (c *Campaign) transplantEdges(from, to domain.Entity) (domain.Facts, domain.Facts) {
	inEdges := c.kb.GetIncomingEdges(from)
	newRels := []domain.Relation{}
	removeRels := []domain.Relation{}

	// Update the relation to point 'from' to the old 'to' the new entity
	for _, rel := range inEdges {
		removeRels = append(removeRels, rel)

		if relocator, ok := rel.(domain.RelationRelocator); ok {
			newRel := relocator.WithTarget(to)
			newRels = append(newRels, newRel)
		} else {
			slog.Warn("Could not relocate incoming relation", "relation", rel.GetRelationName())
		}
	}

	for _, rel := range c.kb.GetOutgoingEdges(from) {
		removeRels = append(removeRels, rel)
		// newRel := domain.WithSourceOf(rel, to.GetId())
		// newRels = append(newRels, newRel)

		if relocator, ok := rel.(domain.RelationRelocator); ok {
			newRel := relocator.WithSource(to)
			newRels = append(newRels, newRel)
		} else {
			slog.Warn("Could not relocate outgoing relation", "relation", rel.GetRelationName())
		}
	}

	return domain.Facts{Relations: newRels}, domain.Facts{Relations: removeRels}
}

func mergeAnalyzedFacts(
	entities map[string]domain.Entity,
	identities map[string]domain.Identity,
	relations *[]domain.Relation,
	assets *[]domain.Asset,
	new domain.Facts,
) {
	for _, e := range new.Entities {
		if ex, ok := entities[e.GetId()]; ok {
			e = domain.UpdateEntity(e, ex)
		}
		entities[e.GetId()] = e
	}
	for _, id := range new.Identities {
		identities[id.GetId()] = id
	}
	*relations = append(*relations, new.Relations...)
	*assets = append(*assets, new.Assets...)
}

// analyzePod collects all per-pod facts and returns them to the caller.
// No global mutation here; the caller merges & enqueues nf.Entities.
func (c *Campaign) analyzePod(e domain.Pod) (domain.Facts, error) {
	entities := make(map[string]domain.Entity)
	relations := make([]domain.Relation, 0, 8)

	// 1) ensure pod is up-to-date
	// Capture IsRunning before the merge: false is a meaningful value but
	// UpdateEntity's merge treats it as zero/unset and would restore the old true.
	incomingIsRunning := e.IsRunning
	if prev, ok := c.GetEntityById(e.GetId()); ok {
		e = domain.UpdateEntity(e, prev).(domain.Pod)
		if !incomingIsRunning && e.IsRunning {
			e.IsRunning = false
		}
	}
	entities[e.GetId()] = e

	// 2) node relation (+ node entity)
	if e.NodeName != "" {
		node := domain.NewK8sNode(e.NodeName)
		if _, exists := entities[node.GetId()]; !exists {
			entities[node.GetId()] = node
		} else {
			entities[node.GetId()] = domain.UpdateEntity(entities[node.GetId()], node)
		}
		if e.IsRunning {
			e.RunsOn = &node
			relations = append(relations, domain.RunsOn{Pod: e, Node: node})
		}
	}

	// 3) service account relation (+ SA entity)
	if e.ServiceAccountName != "" {
		sa := domain.NewServiceAccount(e.ServiceAccountName, e.GetNamespace())
		entities[sa.GetId()] = sa
		if e.AutomountServiceAccountToken.Bool() {
			relations = append(relations, domain.Uses{
				SubjectId: e.GetId(),
				ObjectId:  sa.GetId(),
			})
		} else {
			// relations = append(relations, domain.Reference{
			// 	Source: e.GetId(),
			// 	Target: sa.GetId(),
			// })
		}
	}

	for _, file := range e.MissingFiles {
		if file == "/var/run/secrets/kubernetes.io/serviceaccount/token" {
			e.AutomountServiceAccountToken.Update(-0.3) // the SA Token is probably not automounted
			entities[e.GetId()] = e
		}
	}

	// 4) mounts
	if mf, err := analyzeMountInfo(e); err != nil {
		slog.Error("Failed to analyze MountInfo", "error", err)
	} else {
		for _, ent := range mf.Entities {
			// prefer newer details if we already put something in localEntities
			if ex, ok := entities[ent.GetId()]; ok {
				ent = domain.UpdateEntity(ent, ex)
			}
			entities[ent.GetId()] = ent
		}
		relations = append(relations, mf.Relations...)
	}

	// 5) host path (keeps your current behavior of only warning on error)
	if _, err := analyzeHostPath(e); err != nil {
		slog.Warn("Failed to analyze host path for pod", "error", err, "pod", e.GetId())
	}

	// 6) host relations
	// ensure latest pod (with possible updates above)
	e = entities[e.GetId()].(domain.Pod)
	hostRels := analyzePodHostRelations(e)
	relations = append(relations, hostRels...)

	// 7) workload ownership - match this pod against all existing workloads
	workloads := c.getWorkloads()
	for _, workload := range workloads {
		workloadRels := matchWorkloadToPods(workload, []domain.Pod{e})
		relations = append(relations, workloadRels...)
	}

	// 8) pack results
	ents := make([]domain.Entity, 0, len(entities))
	for _, v := range entities {
		ents = append(ents, v)
	}
	return domain.Facts{
		Entities:   ents,
		Relations:  relations,
		Assets:     nil,
		Identities: nil,
	}, nil
}

func (c Campaign) analyzeSelfSubjectRulesReview(ssrr domain.SelfSubjectRulesReview) (domain.Facts, domain.Facts, error) {
	entities := make([]domain.Entity, 0)
	relations := make([]domain.Relation, 0)

	sa := ssrr.ServiceAccount
	ns := domain.NewNamespace(sa.Namespace)
	sa.Namespace = ns.Name

	entitlements := make([]domain.RBACPermission, 0, len(ssrr.ResourceRules)+len(ssrr.NonResourceRules))

	for _, rule := range ssrr.ResourceRules {
		for _, verb := range rule.Verbs {
			for _, resource := range rule.Resources {
				for _, apiGroup := range rule.APIGroups {
					nsName := ""

					if IsNamespacedResource(resource, apiGroup) {
						nsName = sa.Namespace
					}

					if len(rule.ResourceNames) == 0 {
						perm := domain.RBACPermission{
							Verb:         verb,
							ResourceType: resource,
							APIGroup:     apiGroup,
							Scope:        nsName,
						}
						entitlements = append(entitlements, perm)
					} else {
						for _, resourceName := range rule.ResourceNames {
							perm := domain.RBACPermission{
								Verb:         verb,
								ResourceType: resource,
								ResourceName: resourceName,
								APIGroup:     apiGroup,
								Scope:        nsName,
							}
							entitlements = append(entitlements, perm)
						}
					}
				}
			}
		}
	}

	for _, rule := range ssrr.NonResourceRules {
		for _, verb := range rule.Verbs {
			for _, url := range rule.NonResourceURLs {
				entitlements = append(entitlements, domain.RBACPermission{
					Verb:         verb,
					ResourceName: url,
					// Scope:        sa.GetNamespace(),
				})
			}
		}
	}
	sa.Entitelements = entitlements

	entities = append(entities, ns, sa)
	return domain.Facts{
		Entities:  entities,
		Relations: relations,
	}, domain.Facts{}, nil
}

// IsNamespacedResource returns true if the given Kubernetes resource name is namespaced.
// If the resource is unknown, it defaults to true (namespaced).
// Wildcards ("*") cover both namespaced and cluster-scoped resources, so they
// are treated as cluster-scoped (returns false) to avoid restricting scope.
func IsNamespacedResource(resourceName string, apiGroup string) bool {
	// Wildcards span both namespaced and cluster-scoped resources
	if resourceName == "*" || apiGroup == "*" {
		return false
	}

	// Cluster-scoped resources keyed by apiGroup ("" = core/v1).
	// Derived from: kubectl api-resources --namespaced=false -o json
	nonNamespaced := map[string]map[string]struct{}{
		"": { // core group
			"componentstatuses": {},
			"componentstatus":   {},
			"namespaces":        {},
			"namespace":         {},
			"nodes":             {},
			"node":              {},
			"persistentvolumes": {},
			"persistentvolume":  {},
		},
		"admissionregistration.k8s.io": {
			"mutatingwebhookconfigurations":     {},
			"mutatingwebhookconfiguration":      {},
			"validatingadmissionpolicies":       {},
			"validatingadmissionpolicy":         {},
			"validatingadmissionpolicybindings": {},
			"validatingadmissionpolicybinding":  {},
			"validatingwebhookconfigurations":   {},
			"validatingwebhookconfiguration":    {},
		},
		"apiextensions.k8s.io": {
			"customresourcedefinitions": {},
			"customresourcedefinition":  {},
		},
		"apiregistration.k8s.io": {
			"apiservices": {},
			"apiservice":  {},
		},
		"authentication.k8s.io": {
			"selfsubjectreviews": {},
			"selfsubjectreview":  {},
			"tokenreviews":       {},
			"tokenreview":        {},
		},
		"authorization.k8s.io": {
			"selfsubjectaccessreviews": {},
			"selfsubjectaccessreview":  {},
			"selfsubjectrulesreviews":  {},
			"selfsubjectrulesreview":   {},
			"subjectaccessreviews":     {},
			"subjectaccessreview":      {},
		},
		"certificates.k8s.io": {
			"certificatesigningrequests": {},
			"certificatesigningrequest":  {},
		},
		"flowcontrol.apiserver.k8s.io": {
			"flowschemas":                 {},
			"flowschema":                  {},
			"prioritylevelconfigurations": {},
			"prioritylevelconfiguration":  {},
		},
		"networking.k8s.io": {
			"ingressclasses": {},
			"ingressclass":   {},
			"ipaddresses":    {},
			"ipaddress":      {},
			"servicecidrs":   {},
			"servicecidr":    {},
		},
		"node.k8s.io": {
			"runtimeclasses": {},
			"runtimeclass":   {},
		},
		"rbac.authorization.k8s.io": {
			"clusterrolebindings": {},
			"clusterrolebinding":  {},
			"clusterroles":        {},
			"clusterrole":         {},
		},
		"resource.k8s.io": {
			"deviceclasses":  {},
			"deviceclass":    {},
			"resourceslices": {},
			"resourceslice":  {},
		},
		"scheduling.k8s.io": {
			"priorityclasses": {},
			"priorityclass":   {},
		},
		"storage.k8s.io": {
			"csidrivers":              {},
			"csidriver":               {},
			"csinodes":                {},
			"csinode":                 {},
			"storageclasses":          {},
			"storageclass":            {},
			"volumeattachments":       {},
			"volumeattachment":        {},
			"volumeattributesclasses": {},
			"volumeattributesclass":   {},
		},
	}

	name := strings.ToLower(resourceName)
	group := strings.ToLower(apiGroup)

	if groupMap, ok := nonNamespaced[group]; ok {
		if _, found := groupMap[name]; found {
			return false
		}
	}
	return true
}

// Extract interesting facts from the environment variables.
// Kubernetes provides useful information as environment variables by default, such as:
// - the name of the pod
// - Kube-API endpoint
// - A list of all services that were running when a Container was created is available to that Container as environment variables.
// - if the `enableServiceLinks` flag is set
// - this is limited to services within the same namespace as the new Container's Pod and Kubernetes control plane services
// See [docs: Container Environmeent](https://kubernetes.io/docs/concepts/containers/container-environment/) for more.
// :param event: EnvironmentVariablesReceived  event with the source system and the variables
// returns a new event with new facts, if there were any
func analyzeEnvironmentVariables(source domain.Entity, envVars map[string]string) (domain.Facts, domain.Facts, error) {
	entities := make([]domain.Entity, 0)
	relations := make([]domain.Relation, 0)

	// TODO: how can it be inferred, if it's from a K8s pod vs any *nix-based system?

	srcPod := source.(domain.Pod)
	hostname, found := envVars["HOSTNAME"]
	if found {
		srcPod.HostName = hostname
	}

	nsName := srcPod.GetNamespace()
	srcPod.EnvVars = envVars
	entities = append(entities, srcPod)

	// TODO: parse variables ending with '.svc.cluster.local'

	// podName = get(event.variables, "HOSTNAME", "?")
	// ns = Namespace(name="?")
	// # TODO: env vars don't imply it's in a Pod?
	// pod = System(id=event.sourceSystemId, name=podName, ns=ns)

	// services = getServicesFromEnvVars(event.variables)
	services := getServicesFromEnvVars(envVars)

	for svcName, info := range services {
		rel := domain.Reference{
			Source: srcPod.GetId(),
		}
		if svcName == "KUBERNETES" {
			kubeSystemNs := domain.NewNamespace("kube-system")
			entities = append(entities, kubeSystemNs)

			p := domain.NewPod("api-server", kubeSystemNs.Name)
			p.IPs = append(p.IPs, net.IPAddr{IP: net.ParseIP(info.host)})

			apiServer := domain.ApiServer{Pod: p}
			rel.Target = apiServer.GetId()
			entities = append(entities, apiServer)
		} else {
			svc := domain.Service{
				K8sEntity: domain.K8sEntity{
					Name:      svcName,
					Namespace: nsName,
					Kind:      "Service",
				},
				Host:  info.host,
				Ports: info.ports,
			}
			//         # services are either from same namespace as the pod, or 'kube-system'; assume same namespace as pod for now
			//         # TODO check if there are other services from the kube-system NS, which are added as env_var
			//         sys = Service(name=svc, ip=data["host"], ports=data["ports"], ns=ns)
			entities = append(entities, svc)
			rel.Target = svc.GetId()
		}
		relations = append(relations, rel)
	}

	// TODO check for credentials and add them as assets
	// # TODO: analyze if URL is K8s DNS specific
	// return NewFacts(entities=entities, relations=relations, assets=[])

	return domain.Facts{
		Entities:  entities,
		Relations: relations,
	}, domain.Facts{}, nil
}

func analyzeServiceAccountToken(token domain.ServiceAccountToken) (domain.Facts, error) {
	parts := strings.SplitN(token.Raw, ".", 3)
	newFacts := domain.Facts{}

	if len(parts) != 3 {
		return newFacts, errors.New("invalid token format")
	}
	/*
		# add max of padding before decoding in case padding is missing (
		# extra padding will be ignored by Python's b64decode function anyways
	*/
	encPayload := parts[1]
	payloadData, err := base64.RawStdEncoding.DecodeString(encPayload)
	if err != nil {
		return newFacts, err
	}

	saToken := domain.ServiceAccountToken{}
	if err := json.Unmarshal(payloadData, &saToken); err != nil {
		return newFacts, err
	}
	saToken.Raw = token.Raw
	// a token is bound if there is a Pod (and node) in the priveat kubernetes.io claim
	// see: https://kubernetes.io/docs/reference/access-authn-authz/service-accounts-admin/#bound-service-account-tokens
	saToken.IsBound = saToken.Kubernetes.Pod.UID != ""

	ns := domain.NewNamespace(saToken.Kubernetes.Namespace)
	sa := domain.NewServiceAccount(saToken.Kubernetes.ServiceAccount.Name, ns.Name)
	sa.Token = saToken

	pod := domain.NewPod(saToken.Kubernetes.Pod.Name, ns.Name)
	pod.UID = saToken.Kubernetes.Pod.UID
	pod.ServiceAccountName = sa.Name
	saUsage := domain.Uses{
		SubjectId: pod.GetId(),
		ObjectId:  sa.GetId(),
	}
	nsContainsSa := domain.Contains{
		Container: ns,
		Object:    sa,
	}

	// bound SA tokens also provide information on the node it is running on
	node := domain.NewK8sNode(saToken.Kubernetes.Node.Name)
	node.UID = saToken.Kubernetes.Node.UID
	pod.NodeName = node.Name
	pod.RunsOn = &node

	return domain.Facts{
		Entities:  []domain.Entity{ns, sa, pod, node},
		Assets:    []domain.Asset{saToken},
		Relations: []domain.Relation{saUsage, nsContainsSa}, // nodeRunsPod, podRunsOnNode},
	}, nil
}

func analyzeFailedTTPExecution(ev domain.TTPExecuted) (domain.Facts, domain.Facts, string, error) {
	var failReason string

	if len(ev.Results) == 0 {
		return domain.Facts{}, domain.Facts{}, failReason, fmt.Errorf("no results found for TTP execution %s, so nothing to analyze", ev.TTP.ID)
	}

	entities := make([]domain.Entity, 0)
	relations := make([]domain.Relation, 0)

	// the tool part of the procedure was not on the target system
	new, _, msg, err := analyzeToolSuccessfullyUsedInTTP(ev)
	if err == nil {
		entities = append(entities, new.Entities...)
		if msg != "" {
			failReason = msg
		}
	} else {
		return domain.Facts{}, domain.Facts{}, failReason, err
	}

	new, err = parsePSSViolation(ev.Target, ev.Args, ev.Results)
	if err != nil {
		slog.Error(fmt.Sprintf("Failed to parse PodSecurity violation from error message: %s", err.Error()))
	} else if len(new.Entities) > 0 {
		entities = append(entities, new.Entities...)
		relations = append(relations, new.Relations...)
		failReason = "Namespace enforces a PSS"
	}

	for _, errMsg := range ev.Results {
		if strings.TrimSpace(errMsg) == "" {
			continue
		}

		// TTP failed at the Kubernetes API server for various reasons (RBAC, admission control, etc.)
		// find out which type of error it was
		if strings.Contains(errMsg, "Error from server (Forbidden)") {
			// examples:
			// "command terminated with exit code 1: 'Error from server (Forbidden): pods is forbidden: User \"system:serviceaccount:dev:default\" cannot list resource \"pods\" in API group \"\" in the namespace \"dev\"\n'"
			if strings.Contains(errMsg, "is forbidden: User") { // heuristic: use this as hint for RBAC issue
				if sa, err := parseViolatingRBACIdentity(errMsg); err == nil {
					entities = append(entities, sa)
					failReason = fmt.Sprintf("%s has not the permissions to %s", sa.GetId(), formatRBACPermissions(ev.TTP.Requires.RBACPermissions))
				} else {
					slog.Error(fmt.Sprintf("Failed to parse RBAC identity from error message: %s", err.Error()))
				}
			}

		}

		// Target pod was deleted — the k8s API returns 404 "pods \"<name>\" not found".
		// Mark it as no longer running so syncCapabilities removes the can-access relation.
		if strings.Contains(errMsg, "Status 404: pods ") && strings.Contains(errMsg, "not found") {
			if pod, ok := ev.Target.(domain.Pod); ok {
				pod.IsRunning = false
				entities = append(entities, pod)
				failReason = fmt.Sprintf("Target pod '%s' no longer exists", pod.GetName())
			}
		}

		// // TODO: check if the actual TTP execution failed, because the role already exists
		// // -> overall, the intended effects are met, but it may be a confiict (e.g. name collision), for downstream TTPs
		// if strings.Contains(ev.Results[0], "Error from server (Forbidden)") {
		// 	// "command terminated with exit code 1: 'Error from server (Forbidden): roles.rbac.authorization.k8s.io \"nsadmin\" is forbidden: user \"system:serviceaccount:dev:developer\" (groups=[\"system:serviceaccounts\" \"system:serviceaccounts:dev\" \"system:authenticated\"]) is attempting to grant RBAC permissions not currently held:\n{APIGroups:[\"\"], Resources:[\"*\"], Verbs:[\"*\"]}\n'"
		// 	if strings.Contains(ev.Results[0], "attempting to grant RBAC permissions not currently held") {
		// 		return nil, nil, errors.New(ev.Results[0])
		// 	}
		// }

		var failedToWrite bool

		// TODO: generalize the "failed to to write type of errors and the inferred implications"
		if strings.Contains(errMsg, "command terminated with exit code") {
			tool := ev.Procedure.GetTool()

			switch tool {
			case "curl":
				failedToWrite = ev.ExitCode == 23
				failReason = "An error occurred when writing received data to a local file"
			}
		}
		if strings.Contains(errMsg, " is not writeable") {
			failedToWrite = true
		}

		// TTP specific error: could not transfer tool to the target system
		if failedToWrite {
			// TODO: depending on the User ID, it can be either because it's not root, or because the FS is read-only
			target := ev.Target

			if p, ok := target.(domain.Pod); ok {
				p.ReadOnlyRootFilesystem.Update(.3) // belief update that this flag is set
				entities = append(entities, p)
			} else {
				panic(fmt.Sprintf("TTP '%s' executed on non-pod target '%s'", ev.TTP.ID, target.GetId()))
			}
		}

		if strings.Contains(errMsg, ": No such file or directory") {
			if len(entities) > 0 {
				if sys, ok := entities[0].(domain.System); ok {
					file := strings.TrimSpace(strings.Split(errMsg, ":")[1])
					sys.AddMissingFiles(file)
					// replace original entity with updated information
					entities = append(entities[:0], sys)
				}
			}
		}
	}

	// Malformed procedure:
	// example:
	// "command terminated with exit code 1: 'error: error parsing STDIN: json: offset 138: invalid character '$' looking for beginning of value\n'"

	return domain.Facts{
		Entities:  entities,
		Relations: relations,
	}, domain.Facts{}, failReason, nil
}

func parseViolatingRBACIdentity(msg string) (domain.Entity, error) {
	// example: "User \"system:serviceaccount:dev:default\" cannot list resource \"pods\" in API group \"\" in the namespace \"dev\""
	re := regexp.MustCompile(`User\s+"system:serviceaccount:([^:"]+):([^"]+)"`)
	matches := re.FindStringSubmatch(msg)
	if len(matches) != 3 {
		return nil, fmt.Errorf("could not parse RBAC identity from message: %s", msg)
	}
	ns := matches[1]
	saName := matches[2]
	return domain.NewServiceAccount(saName, ns), nil
}

func parsePSSViolation(target domain.Entity, args map[string]string, results []string) (domain.Facts, error) {
	// example: violating PSA: "command terminated with exit code 1: 'Error from server (Forbidden): error when creating "STDIN": pods "bad-pod" is forbidden: violates PodSecurity "baseline:latest": hostPath volumes (volume "hostmount"), privileged (container "bad-pod" must not set securityContext.privileged=true)'"

	entities := make([]domain.Entity, 0)
	// Check if the error message contains information about pod security violations
	podSecurityViolationPattern := regexp.MustCompile(`violates PodSecurity "([^"]+)": (.+)`)
	for _, res := range results {
		matches := podSecurityViolationPattern.FindStringSubmatch(res)
		if len(matches) >= 3 {
			securityProfile := matches[1] // e.g., "baseline:latest"
			violationReason := matches[2] // e.g., "privileged ..."
			slog.Error(fmt.Sprintf("Pod creation failed due to security violation - Profile: %s, Reason: %s",
				securityProfile, violationReason))

			if t, ok := target.(domain.Namespaced); ok {
				ns := domain.Namespace{Name: t.GetNamespace(), EnforcedPSS: securityProfile, Kind: "Namespace"}
				entities = append(entities, ns)
			} else if ns, ok := target.(domain.Namespace); ok {
				ns.EnforcedPSS = securityProfile
				entities = append(entities, ns)
			} else if nsName, ok := args["Namespace"]; ok {
				ns := domain.Namespace{Name: nsName, EnforcedPSS: securityProfile, Kind: "Namespace"}
				entities = append(entities, ns)
			} else {
				slog.Error("No namespace found in event target or args")
			}
		}
	}

	return domain.Facts{Entities: entities}, nil
}

type ServiceInfo struct {
	host  string
	ports map[string]int
}

// Extract services from the environment variables.
// To extract services automatically populated by Kubernetes, a simple heuristic is used.
// (see: https://kubernetes.io/docs/concepts/services-networking/service/#environment-variables)
//  1. look for all entries ending with `<xyz>_SERVICE_HOST`, the leading `<xyz>` is the service name
//  2. for all service names get all other environment variables starting with this name
//  3. get the host by reading the `<xyz>_SERVICE_HOST` value
//  4. get all named ports by reading `<xyz>_SERVICE_PORT_<portname>`
//     - if no named port was found, read `<xyz>_SERVICE_PORT` directly, which is the port number
//
// Group the dict of variables to a single entry with key kUBERNETES and value {host="10.96.0.1" and ports={"HTTPS": 443}}
// ```
//
//	{
//	  'KUBERNETES_PORT': 'tcp://10.96.0.1:443',
//	  'KUBERNETES_PORT_443_TCP': 'tcp://10.96.0.1:443',
//	  'KUBERNETES_PORT_443_TCP_ADDR': '10.96.0.1',
//	  'KUBERNETES_PORT_443_TCP_PORT': '443',
//	  'KUBERNETES_PORT_443_TCP_PROTO': 'tcp',
//	  'KUBERNETES_SERVICE_HOST': '10.96.0.1',
//	  'KUBERNETES_SERVICE_PORT': '443',
//	  'KUBERNETES_SERVICE_PORT_HTTPS': '443',
//	}```
//
// :param variables: a dict of environment variables and their values
// :return: a dict of services, with the service name as the key, and a dict with `host` and `ports` as its value
func getServicesFromEnvVars(vars map[string]string) map[string]ServiceInfo {
	const SVC_HOST_SFX = "_SERVICE_HOST"
	const SVC_HOST = "SERVICE_HOST"
	const SVC_PORT = "SERVICE_PORT"

	serviceNames := make([]string, 0)
	// serviceNames := make(map[string]string)
	for k := range vars {
		if strings.HasSuffix(k, SVC_HOST_SFX) {
			name := strings.Replace(k, SVC_HOST_SFX, "", 1)
			serviceNames = append(serviceNames, name)
		}
	}

	svcGroups := make(map[string]ServiceInfo)
	for _, svcName := range serviceNames {
		grp := make(map[string]string, 0)
		for v, value := range vars {
			if strings.HasPrefix(v, svcName) {
				entry := strings.Replace(v, svcName+"_", "", 1)
				grp[entry] = value
			}
		}

		host, ok := grp[SVC_HOST]
		if !ok {
			slog.Error(fmt.Sprintf("No host found for service '%s'", svcName))
		}

		// TODO  also support the PROTO specifiied as environment variable
		ports := make(map[string]int)
		for k, v := range grp {
			if strings.HasPrefix(k, SVC_PORT+"_") {
				port, err := strconv.Atoi(v)
				if err != nil {
					slog.Error(fmt.Sprintf("Can't convert Port %s to int", v))
				} else {
					ports[strings.Replace(k, SVC_PORT+"_", "", 1)] = port
				}
			}
		}

		// If no named port was present use the SERVICE_PORT variable, which should always exist
		if len(ports) == 0 {
			p := grp[SVC_PORT]
			port, err := strconv.Atoi(p)
			if err != nil {
				slog.Error(fmt.Sprintf("Can't convert Port %s to int", p))
			} else {
				ports[""] = port
			}
		}

		svcGroups[svcName] = ServiceInfo{host: host, ports: ports}
	}

	return svcGroups
}

// function analyzeExtractedServiceAccountToken(event::ServiceAccountTokenExtracted)::Union{NewFacts,Nothing}
//     header, encData, signature = split(event.rawToken, ".")
//     # add max of padding before decoding in case padding is missing (
//     # extra padding will be ignored by Python's b64decode function anyways

//     # payload_data = base64.b64decode(enc_payload + "==").decode("utf-8")

//     # TODO: maybe pad to next multiple of 4 (if necessary)
//     # rpad(data, MULTPILE?, "=")
//     data = String(base64decode(encData))

//     payload = JSON3.read(data)
//     # payload = json.loads(data)
//     k8sInfo = payload["kubernetes.io"]
//     podInfo = k8sInfo["pod"]
//     saInfo = k8sInfo["serviceaccount"]

//     jwt = JWTToken(
//         subject=payload["sub"],
//         audience=payload["aud"],
//         issuer=payload["iss"],
//         expiresAt=payload["exp"],
//         issuedAt=payload["iat"],
//         notValidBefore=payload["nbf"],
//         raw=event.rawToken
//     )

//     token = ServiceAccountToken(
//         jwtToken=jwt,
//         namespace=k8sInfo["namespace"],
//         podName=k8sInfo["pod"]["name"],
//         podUid=k8sInfo["pod"]["uid"],
//         serviceAccountName=k8sInfo["serviceaccount"]["name"],
//         serviceAccountUid=k8sInfo["serviceaccount"]["uid"],
//         warnAfter=k8sInfo["warnafter"],
//     )

//     ns = Namespace(name=k8sInfo["namespace"])
//     sa = ServiceAccount(name=saInfo["name"], ns=ns, token=token, expiresAt=payload["exp"])
//     # TODO check: if SA tokens can target other pods then the system where it was mounted on?
//     pod = Pod(id=event.sourceSystemId, name=podInfo["name"], ns=ns, serviceAccount=sa)
//     # pod.service_account = sa

//     sa_usage = Relation(name="uses", source=pod.id, destination=sa.id)
//     # TODO: add token to loot (with ref to the system)
//     # - extract the namespace, SA name and pod name (if necessary?)
//     # - update topology and add parent node being the namespace (if not yet set)
//     # - set `kind` of the system
//     # - send updated topology to the UI
//     #   - add the SA token as a small entity
//     #   - everything is in NS compound node

//     return NewFacts(
//         entities=[
//             ns,
//             sa,
//             pod,
//         ],
//         assets=[token],
//         relations=[sa_usage],
//     )

// }

func extractHostPaths(mounts []domain.Mount) []domain.Mount {
	hostMounts := make([]domain.Mount, 0)
	// Define keywords that suggest host mount exposure
	// hostPathIndicators := []string{
	// 	"/var/lib/kubelet",
	// 	"/etc/hostname",
	// 	"/etc/resolv.conf",
	// 	"/dev/vda", // common block device prefix
	// }

	// fsTypeIndicators := []string{
	// 	"xfs", "ext4", "btrfs", // real filesystems
	// }

	// for _, mount := range mounts {
	// 	for _, keyword := range hostPathIndicators {
	// 		if strings.Contains(mount.MountPoint, keyword) {
	// 			// extract the host mount point
	// 			hostMounts = append(hostMounts, mount)
	// 			// return "/mnt/host", true
	// 		}
	// 	}
	// 	// // Additional check: mounted from real device and looks like /dev/vda or similar
	// 	// postParts := strings.Fields(postSeparator)
	// 	// if len(postParts) >= 2 {
	// 	// 	source := postParts[1]
	// 	// 	if strings.HasPrefix(source, "/dev/") && strings.HasPrefix(mountPoint, "/") {
	// 	// 		return mountPoint, true
	// 	// 	}
	// 	// }
	// 	// Check if mount is backed by a real device
	// 	for _, fs := range fsTypeIndicators {
	// 		// if fsType == fs && strings.HasPrefix(source, "/dev/") {
	// 		if mount.Type == fs && strings.HasPrefix(mount.MountPoint, "/dev/") {
	// 			hostMounts = append(hostMounts, mount)
	// 		}
	// 	}
	// }

	return hostMounts
}

func analyzeMountInfo(system domain.System) (domain.Facts, error) {
	entities := make([]domain.Entity, 0)
	relations := make([]domain.Relation, 0)

	var node domain.K8sNode
	nodeName := "?"
	var foundNode bool
	var srcPod domain.Pod

	trackedHostPaths := map[string]bool{} // to avoid duplicates

	if pod, ok := system.(domain.Pod); ok && pod.NodeName != "" {
		nodeName = pod.NodeName
	}

	for _, mount := range system.GetMounts() {
		// kubelet-related paths are clearly part of the Node filesystem
		if strings.Contains(mount.MountPoint, "/var/lib/kubelet") {
			// one time logic to establish
			if !foundNode {
				foundNode = true
				node = domain.NewK8sNode(nodeName)
				if pod, ok := system.(domain.Pod); ok {
					srcPod = pod
					srcPod.RunsOn = &node
					relations = append(relations, domain.RunsOn{
						Pod:  pod,
						Node: node,
					})
				} else {
					slog.Error("system is not of type domain.Pod")
				}
			}

			// if the mount point doesn't start with /var/lib, then it must be a hostpath
			if !strings.HasPrefix(mount.MountPoint, "/var/lib/kubelet/pods/") {
				if i := strings.Index(mount.MountPoint, "/var/lib/kubelet/"); i >= 0 {
					hostPath, saTokenPath := mount.MountPoint[:i], mount.MountPoint[i:]+"/token"
					node.SystemImpl.Files = append(node.SystemImpl.Files, saTokenPath)
					mount.HostPath = hostPath
					if _, exists := trackedHostPaths[hostPath]; exists {
					} else if srcPod.Name != "" {
						srcPod.VolumeMounts = append(srcPod.VolumeMounts, domain.Mount{
							MountRoot:  mount.MountRoot,
							MountPoint: hostPath,
							IsHostPath: true,
						})
						trackedHostPaths[hostPath] = true
					}
				}
			}

			if managedPod, err := createPodFromKubeletMounts(mount); err == nil {
				// // if the pod is already known, update it
				// if existingPod, ok := entities[managedPod.GetId()].(domain.Pod); ok {
				// 	managedPod = domain.UpdateEntity(managedPod, existingPod).(domain.Pod)
				// }
				entities = append(entities, managedPod)
				relations = append(relations, domain.RunsOn{
					Pod:  managedPod,
					Node: node,
				})
				slog.Debug(fmt.Sprintf("Created pod from kubelet mount: %s", managedPod.GetId()))
			} else {
				slog.Warn("Created pod from kubelet mount has no name")
			}
		}
	}

	// TODO analyze the kubelet files on the node

	if foundNode {
		entities = append(entities, node)
	}
	if srcPod.Kind != "" { // Kind is automtically set when using a constructor like NewPod
		entities = append(entities, srcPod)
	}

	return domain.Facts{
		Entities:  entities,
		Relations: relations,
	}, nil
}

func createPodFromKubeletMounts(mount domain.Mount) (domain.Pod, error) {
	p := domain.NewPod("", "")
	// extract the UID from the mount point, which is between '/kubelet/pods/' and '/volumens'
	// remove the prefix to make it agnostic to the hostpath
	// as a result the UID is the first part of the string
	path := strings.TrimPrefix(mount.MountPoint, mount.HostPath+"/var/lib/kubelet/pods/")
	parts := strings.Split(path, "/")
	if len(parts) < 2 {
		return p, fmt.Errorf("invalid mount point format: %s", mount.MountPoint)
	}
	podUID := parts[0]
	if podUID == "" {
		return p, fmt.Errorf("no pod UID found in mount point: %s", mount.MountPoint)
	}

	if err := uuid.Validate(podUID); err != nil {
		return p, fmt.Errorf("pod UID is not a valid UUID: %s", podUID)
	} else {
		p.UID = podUID
	}

	return p, nil
}

func analyzeHostPath(pod domain.Pod) (domain.Facts, error) {
	entities := make([]domain.Entity, 0)
	relations := make([]domain.Relation, 0)

	hostPaths := extractHostPaths(pod.GetMounts()) // TODO: handle multiple results, if necessary

	if len(hostPaths) > 0 {
		pod.VolumeMounts = append(pod.VolumeMounts, hostPaths...)

		slog.Debug(fmt.Sprintf("Host path found: %s", hostPaths[0].MountPoint))
		// pod := ev.Target.(domain.Pod)
		// pod.VolumeMounts = append(pod.VolumeMounts, domain.Mount{
		// 	Root:       hostPath,
		// 	MountPath:  ev.Args["MountPath"],
		// 	IsHostPath: true,
		// })
		// entities = append(entities, pod)

		// rel := domain.MountsHostPath{
		// 	Pod:       pod,
		// 	MountPath: ev.Args["MountPath"],
		// 	HostPath:  hostPath,
		// 	Node:      domain.NewK8sNode(pod.NodeName),
		// }
		// relations = append(relations, rel)
	} else {
		slog.Debug("No host path found in the results")
	}

	//  if ev.Target == nil {
	//   return NewFacts{}, RemovedFacts{}, errors.New("no target found for host path TTP execution")
	//  }

	//  if p, ok := ev.Target.(domain.Pod); ok {
	//   p.HostPath = ev.Args["HostPath"]
	//   p.MountPath = ev.Args["MountPath"]
	//   entities = append(entities, p)

	//   rel := domain.MountsHostPath{
	//    Pod:       p,
	//    HostPath:  ev.Args["HostPath"],
	//    MountPath: ev.Args["MountPath"],
	//   }
	//   relations = append(relations, rel)
	//  } else {
	//   slog.Error(fmt.Sprintf("TTP '%s' executed on non-pod target '%s'", ev.TTP.ID, ev.Target.GetId()))
	//  }

	return domain.Facts{
		Entities:  entities,
		Relations: relations,
	}, nil
}

func analyzePodHostRelations(pod domain.Pod) []domain.Relation {
	rels := make([]domain.Relation, 0)

	if len(pod.VolumeMounts) > 0 {
		var node domain.K8sNode

		if pod.RunsOn == nil {
			node = domain.NewK8sNode(pod.NodeName)
		} else {
			node = *pod.RunsOn
		}
		rel := domain.NewMountsHostPathsRelation(pod, node)

		for _, vm := range pod.VolumeMounts {
			if vm.IsHostPath {
				rel.AddMount(vm.MountPoint, vm.MountRoot)
			}
		}

		if len(rel.HostPaths) > 0 {
			rels = append(rels, rel)
		}
	}

	return rels
}

func isToolExecutionFailure(ttpResults []string, toolName string) bool {
	// TODO: try to parse the tool name from the errormessage
	toolNotFoundMsgs := []string{
		fmt.Sprintf("%s: not found", toolName),
		fmt.Sprintf("%s: command not found", toolName),
		"executable file not found in $PATH", // happened when using `k exec`
	}
	for _, result := range ttpResults {
		for _, toolNotFoundMsg := range toolNotFoundMsgs {
			if strings.Contains(result, toolNotFoundMsg) {
				// "command terminated with exit code 127: 'sh: 1: kubectl: not found\n'"
				// "bash: wget: command not found"  on nginx pod
				return true
			}
		}
	}
	return false
}

func analyzeToolSuccessfullyUsedInTTP(ev domain.TTPExecuted) (domain.Facts, domain.Facts, string, error) {
	// get the system on which the TTP was executed
	newFacts := domain.Facts{}
	var msg string

	tool := ev.Procedure.GetTool()
	isToolFailure := isToolExecutionFailure(ev.Results, tool)

	if execSystem := ev.ExecutedOn; execSystem != nil {
		// add the binary only, if it was not yet known, because just a successful/failed
		// call provides no information of the exact path (which other info sources may do)
		if execSystem.HasBinary(tool).IsUnknown() {
			if ev.Success {
				execSystem.SetBinary(tool, tool)
			} else if isToolFailure {
				execSystem.SetBinary(tool, "") // empty path is a failure
				msg = fmt.Sprintf("Tool '%s' not found on system '%s'", tool, execSystem.GetName())
			}
		}
		newFacts.Entities = append(newFacts.Entities, execSystem)
	}

	return newFacts, domain.Facts{}, msg, nil
}

// shells is the set of common shell interpreters that wrap the actual tool invocation.
var shells = map[string]bool{
	"bash": true, "sh": true, "zsh": true, "dash": true, "ksh": true, "ash": true,
}

// getInvokedTool extracts the actual tool name from a TTPExecuted event.
// It first checks Args for an explicit "TOOL" key, then parses the Procedure.Command
// to find the real binary being invoked, unwrapping shell wrappers like `bash -c "nmap ..."`.
// Falls back to Procedure.GetTool() if no tool can be extracted from the command.
func getInvokedTool(ev domain.TTPExecuted) string {
	if tool, ok := ev.Args["TOOL"]; ok && tool != "" {
		return tool
	}

	cmd := strings.TrimSpace(ev.Procedure.Command)
	if cmd == "" {
		return ev.Procedure.GetTool()
	}

	tool := extractToolFromCommand(cmd)
	if tool == "" {
		return ev.Procedure.GetTool()
	}
	return tool
}

// extractToolFromCommand parses a shell command string and returns the name of the
// actual tool being invoked. It handles:
//   - shell wrappers: `bash -c "nmap -sn 10.0.0.0/24"` → "nmap"
//   - env prefixes: `ENV_VAR=value kubectl get pods` → "kubectl"
//   - command chains: `curl -sS url && chmod +x file` → "curl"
//   - pipes: `cat file | grep pattern` → "cat"
//   - simple commands: `kubectl get pods -n default` → "kubectl"
func extractToolFromCommand(cmd string) string {
	// strip leading env var assignments (e.g. "FOO=bar BAZ=1 kubectl ...")
	for {
		token, rest := nextToken(cmd)
		if token == "" {
			return ""
		}
		if strings.Contains(token, "=") && !strings.HasPrefix(token, "-") {
			cmd = rest
			continue
		}
		break
	}

	first, rest := nextToken(cmd)
	if first == "" {
		return ""
	}

	// strip path prefix: /usr/bin/bash → bash
	base := baseName(first)

	// if it's a shell with -c flag, extract the tool from the inner command
	if shells[base] {
		inner := extractInnerCommand(rest)
		if inner != "" {
			innerTool := extractToolFromCommand(inner)
			if innerTool != "" {
				return innerTool
			}
		}
		// shell used directly (e.g. `sh -i >& /dev/tcp/...`)
		return base
	}

	return base
}

// extractInnerCommand extracts the command string after a -c flag, handling quoting.
// Given `-c "nmap -sn 10.0.0.0/24"`, returns `nmap -sn 10.0.0.0/24`.
// Given `-c 'ls -la'`, returns `ls -la`.
func extractInnerCommand(args string) string {
	args = strings.TrimSpace(args)
	// find -c flag (could be combined like in `bash -xc "cmd"`)
	for {
		token, rest := nextToken(args)
		if token == "" {
			return ""
		}
		if token == "-c" {
			return stripOuterQuotes(strings.TrimSpace(rest))
		}
		// handle combined flags like -xc, -ec
		if strings.HasPrefix(token, "-") && strings.HasSuffix(token, "c") && !strings.HasPrefix(token, "--") {
			return stripOuterQuotes(strings.TrimSpace(rest))
		}
		args = rest
	}
}

// stripOuterQuotes removes a matching pair of outer quotes (single or double) and
// handles escaped quotes within. Returns the inner content.
func stripOuterQuotes(s string) string {
	if len(s) < 2 {
		return s
	}
	if (s[0] == '"' && s[len(s)-1] == '"') || (s[0] == '\'' && s[len(s)-1] == '\'') {
		return s[1 : len(s)-1]
	}
	// if the string starts with a quote but doesn't end with one, find the matching close
	if s[0] == '"' || s[0] == '\'' {
		quote := s[0]
		for i := 1; i < len(s); i++ {
			if s[i] == '\\' {
				i++ // skip escaped char
				continue
			}
			if s[i] == quote {
				return s[1:i]
			}
		}
		// unmatched quote, strip the opening one
		return s[1:]
	}
	return s
}

// nextToken returns the first whitespace-delimited token from s, respecting quotes,
// and the remainder of the string.
func nextToken(s string) (token, rest string) {
	s = strings.TrimSpace(s)
	if s == "" {
		return "", ""
	}

	// handle command chains - stop at && || ; |
	// but process them as delimiters, not part of the token
	i := 0
	inQuote := byte(0)
	for i < len(s) {
		ch := s[i]
		if inQuote != 0 {
			if ch == '\\' && i+1 < len(s) {
				i += 2
				continue
			}
			if ch == inQuote {
				inQuote = 0
			}
			i++
			continue
		}
		if ch == '"' || ch == '\'' {
			inQuote = ch
			i++
			continue
		}
		if ch == ' ' || ch == '\t' {
			return s[:i], s[i:]
		}
		i++
	}
	return s, ""
}

// baseName returns the last path component, like filepath.Base but without importing path.
func baseName(s string) string {
	idx := strings.LastIndex(s, "/")
	if idx >= 0 {
		return s[idx+1:]
	}
	return s
}

// analyzeInvokedTool extracts the actually used tool from the TTP execution and applies
// generic heuristics based on what tool was invoked.
func analyzeInvokedTool(ev domain.TTPExecuted) (domain.Facts, domain.Facts, string, error) {
	tool := getInvokedTool(ev)
	if tool == "" {
		return domain.Facts{}, domain.Facts{}, "", nil
	}

	newFacts := domain.Facts{}

	if execSystem := ev.ExecutedOn; execSystem != nil {
		if execSystem.HasBinary(tool).IsUnknown() {
			if ev.Success {
				execSystem.SetBinary(tool, tool)
			}
		}
		newFacts.Entities = append(newFacts.Entities, execSystem)
	}

	return newFacts, domain.Facts{}, tool, nil
}

func (c Campaign) analyzeRoleBinding(rb domain.RoleBinding) (domain.Facts, domain.Facts, error) {
	entities := []domain.Entity{}
	relations := make([]domain.Relation, 0)

	roleEntityRaw, hasRole := c.GetEntityById(rb.RoleID)
	if !hasRole {
		return domain.Facts{}, domain.Facts{}, fmt.Errorf("role with ID '%s' not found in campaign", rb.RoleID)
	}
	roleEntity, _ := roleEntityRaw.(domain.Role)

	for _, subjectID := range rb.SubjectIDs {
		ns, kind, name, err := UnpackResourceID(subjectID)
		if err != nil {
			slog.Error(fmt.Sprintf("Failed to unpack subject ID '%s': %s", subjectID, err.Error()))
			continue
		}

		var subject domain.Entity
		switch kind {
		case "ServiceAccount":
			subject = domain.NewServiceAccount(name, ns)
		default:
			subject = domain.User{Name: name, Kind: domain.IdentityType(kind)}
		}

		if subj, ok := subject.(domain.Identity); ok {
			relations = append(relations, domain.BindsRole{
				Role:        roleEntity,
				Subject:     subj,
				RoleBinding: rb,
			})
		} else {
			slog.Error(fmt.Sprintf("Subject is not a ServiceAccount: %v", subject))
		}

		entities = append(entities, subject)
	}

	return domain.Facts{
		Entities:  entities,
		Relations: relations,
	}, domain.Facts{}, nil
}

func (c Campaign) analyzeConfigMap(cm domain.ConfigMap) (domain.Facts, domain.Facts, error) {
	entities := []domain.Entity{cm}
	relations := make([]domain.Relation, 0)

	assets := make([]domain.Asset, 0)
	for key, _ := range cm.Data {
		if secret, ok := convertSecret(cm.Name, key, cm.Data[key]); ok {
			assets = append(assets, secret)

			relations = append(relations, domain.ExposesSecret{
				Object: cm,
				Secret: secret,
			})
		}
	}

	return domain.Facts{
		Entities:  entities,
		Relations: relations,
		Assets:    assets,
	}, domain.Facts{}, nil
}

func convertSecret(sourceName, name, value string) (domain.Secret, bool) {
	secretType := domain.Unknown

	secret := domain.Secret{
		Name: name,
		Type: secretType,
		Data: map[string]string{
			name: value,
		},
	}

	return secret, true
}

func (c Campaign) analyzeUnknownSystem(sys domain.UnknownSystem) (domain.Facts, domain.Facts, error) {
	entities := []domain.Entity{}
	relations := make([]domain.Relation, 0)

	knownSystems := c.getSystems(true, false)
	var matchedEntity domain.System

	// check if there is a match based on the hostname?
	for _, knownSys := range knownSystems {
		if isSameSystem(knownSys, sys) {
			matchedEntity = knownSys
			break
		}
	}

	if matchedEntity != nil {
		updatedEntity := domain.UpdateEntity(matchedEntity, sys).(domain.System)
		entities = append(entities, updatedEntity)
	} else {
		isMetadataServer := false
		// check if any of the IPs are the metadata server IP (169.254.169.254)
		for _, ip := range sys.IPs {
			if ip.IP.String() == "169.254.169.254" {
				isMetadataServer = true
				break
			}
		}

		if isMetadataServer {
			// TODO: support other cloud service providers
			csp := "GCP"
			metadataServer := domain.NewMetadataServer(csp)
			entities = append(entities, metadataServer)
		} else {
			// keep the unknown system and try to match it at a later point in time
			entities = append(entities, sys)
		}
	}

	return domain.Facts{
		Entities:  entities,
		Relations: relations,
	}, domain.Facts{}, nil
}

func isSameSystem(a, b domain.System) bool {
	// check different sources of system name against each other
	namesA := map[string]bool{
		a.GetName():     true,
		a.GetHostName(): true,
	}
	namesB := []string{b.GetName(), b.GetHostName()}
	for _, name := range namesB {
		if _, exists := namesA[name]; exists && name != "" {
			return true
		}
	}

	// IP-based matching: if one system is UnknownSystem with an IP/domain ID,
	// compare it against the IPs of the other system
	aUnknown, aIsUnknown := a.(domain.UnknownSystem)
	bUnknown, bIsUnknown := b.(domain.UnknownSystem)

	// Helper function to get IPs from a system
	getSystemIPs := func(sys domain.System) []net.IPAddr {
		// Try to access IPs field via type assertion to implementations
		if pod, ok := sys.(domain.Pod); ok {
			return pod.IPs
		}
		if node, ok := sys.(domain.K8sNode); ok {
			return node.IPs
		}
		if unknown, ok := sys.(domain.UnknownSystem); ok {
			return unknown.IPs
		}
		return nil
	}

	// Case 1: a is UnknownSystem with IP/domain, b is known system
	if aIsUnknown && !bIsUnknown {
		aID := aUnknown.GetId()
		// Check if ID looks like IP or domain (no '/' in the ID)
		if !strings.Contains(aID, "/") {
			// Try to parse as IP
			if ip := net.ParseIP(aID); ip != nil {
				bIPs := getSystemIPs(b)
				for _, bIP := range bIPs {
					if bIP.IP.Equal(ip) {
						return true
					}
				}
			}
		}
	}

	// Case 2: b is UnknownSystem with IP/domain, a is known system
	if bIsUnknown && !aIsUnknown {
		bID := bUnknown.GetId()
		// Check if ID looks like IP or domain (no '/' in the ID)
		if !strings.Contains(bID, "/") {
			// Try to parse as IP
			if ip := net.ParseIP(bID); ip != nil {
				aIPs := getSystemIPs(a)
				for _, aIP := range aIPs {
					if aIP.IP.Equal(ip) {
						return true
					}
				}
			}
		}
	}

	// TODO: incorporate further heuristics to find a matching entity
	return false
}

func choosePreferredPodIdentity(current, existing domain.Pod) (domain.Pod, domain.Pod) {
	currentScore := podIdentityConfidence(current)
	existingScore := podIdentityConfidence(existing)

	if currentScore > existingScore {
		return current, existing
	}

	// Keep the existing pod when confidence is equal to avoid flapping IDs.
	return existing, current
}

func podIdentityConfidence(p domain.Pod) int {
	score := 0
	if p.UID != "" {
		score += 4
	}
	if p.GetNamespace() != "" && p.GetNamespace() != "?" {
		score += 2
	}
	if p.GetName() != "" && !looksLikeGuessedPodName(p.GetName(), p.IPs) {
		score += 2
	}
	if p.ServiceAccountName != "" {
		score += 1
	}
	return score
}

func looksLikeGuessedPodName(name string, ips []net.IPAddr) bool {
	for _, ipAddr := range ips {
		if ipAddr.IP == nil {
			continue
		}
		ipKebab := strings.ReplaceAll(ipAddr.IP.String(), ".", "-")
		if name == "pod-"+ipKebab || name == ipKebab || strings.Contains(name, ipKebab) {
			return true
		}
	}
	return false
}

func podsShareIP(a, b domain.Pod) bool {
	for _, aIP := range a.IPs {
		if aIP.IP == nil {
			continue
		}
		for _, bIP := range b.IPs {
			if bIP.IP == nil {
				continue
			}
			if aIP.IP.Equal(bIP.IP) {
				return true
			}
		}
	}
	return false
}

func removeEntityByID(entities []domain.Entity, id string) []domain.Entity {
	filtered := make([]domain.Entity, 0, len(entities))
	for _, entity := range entities {
		if entity.GetId() == id {
			continue
		}
		filtered = append(filtered, entity)
	}
	return filtered
}

func (c *Campaign) reconcileTokenPodWithSource(token domain.ServiceAccountToken, sourceSystem domain.System, entities []domain.Entity) (domain.Pod, domain.Pod, bool) {
	sourcePod, ok := sourceSystem.(domain.Pod)
	if !ok {
		return domain.Pod{}, domain.Pod{}, false
	}

	tokenPod, found := findTokenPodEntity(entities, token)
	if !found {
		return domain.Pod{}, domain.Pod{}, false
	}

	if sourcePod.GetId() == tokenPod.GetId() {
		return domain.Pod{}, domain.Pod{}, false
	}

	preferred, stale := choosePreferredPodIdentity(tokenPod, sourcePod)
	merged := domain.UpdateEntity(preferred, stale).(domain.Pod)
	return merged, stale, true
}

func findTokenPodEntity(entities []domain.Entity, token domain.ServiceAccountToken) (domain.Pod, bool) {
	for _, entity := range entities {
		pod, ok := entity.(domain.Pod)
		if !ok {
			continue
		}

		if token.Kubernetes.Pod.UID != "" && pod.UID == token.Kubernetes.Pod.UID {
			return pod, true
		}

		if token.Kubernetes.Pod.Name != "" && token.Kubernetes.Namespace != "" {
			if pod.GetName() == token.Kubernetes.Pod.Name && pod.GetNamespace() == token.Kubernetes.Namespace {
				return pod, true
			}
		}
	}
	return domain.Pod{}, false
}

// formatRBACPermissions returns a string representation of a slice of RBACPermission.
func formatRBACPermissions(perms []domain.RBACPermission) string {
	if len(perms) == 0 {
		return "no permissions"
	}
	parts := make([]string, 0, len(perms))
	for _, perm := range perms {
		// Adjust the formatting as needed for your RBACPermission struct
		parts = append(parts, fmt.Sprintf("%s %s %s %s %s", perm.Verb, perm.ResourceType, perm.ResourceName, perm.APIGroup, perm.Scope))
	}
	return strings.Join(parts, "; ")
}

func analyzeDnsEntries(entries map[string]string) (domain.Facts, domain.Facts, error) {
	entities := make([]domain.Entity, 0)
	relations := make([]domain.Relation, 0)

	for ipStr, dnsStr := range entries {
		parts := strings.Split(dnsStr, ".")
		ipKebab := strings.ReplaceAll(ipStr, ".", "-")

		if dnsStr != "" && !strings.HasSuffix(dnsStr, "cluster.local") {
			slog.Info(fmt.Sprintf("Skipping unknown DNS %s", dnsStr))
			continue
		}

		// TODO: extract the namespace from the DNS name
		// use the 4th-to-last label as namespace (e.g. "dev" in "a.b.dev.svc.cluster.local")
		// source: https://kubernetes.io/docs/concepts/services-networking/dns-pod-service/#services
		var ns string
		var name string

		// TODO: support pods hostname/subdomain fields in spec: https://kubernetes.io/docs/concepts/services-networking/dns-pod-service/#pod-hostname-and-subdomain-field

		// Source: https://kubernetes.io/docs/concepts/services-networking/dns-pod-service/#pods
		if len(parts) == 4 {
			// Kube-DNS versions: <pod-IPv4-address>.<namespace>.pod.<cluster-domain>
			name = parts[0]
			ns = parts[len(parts)-4]
		} else if len(parts) == 5 {
			// CoreDNS version: <pod-ipv4-address>.<service-name>.<namespace>.svc.<cluster-domain>
			name = parts[0]
			ns = parts[len(parts)-4]
		} else if len(parts) == 6 {
			// CoreDNS version: <pod-ipv4-address>.<service-name>.<namespace>.svc.<cluster-domain>
			name = parts[1] + "." + parts[0]
			ns = parts[len(parts)-4]
		} else if len(parts) > 2 {
			// fallback to previous behavior if DNS has fewer labels
			ns = parts[2]
		}

		ip := net.ParseIP(ipStr)
		if ip == nil {
			slog.Error(fmt.Sprintf("Invalid IP address: %s", dnsStr))
			continue
		}

		isPod := parts[0] == ipKebab
		if isPod {
			pod := domain.NewPod(name, ns)
			pod.SystemImpl.IPs = append(pod.SystemImpl.IPs, net.IPAddr{IP: ip})
			entities = append(entities, pod)
			// } else if dnsStr == "" {
			// 	sys := domain.NewSystem(ipStr, "", domain.NoAccess)
			// 	sys.IPs = append(sys.IPs, net.IPAddr{IP: ip})
			// 	entities = append(entities, sys)
		} else {
			svc := domain.NewService(name, ns)
			svc.Host = dnsStr
			svc.IP = net.IPAddr{IP: ip}
			entities = append(entities, svc)

		}
	}

	return domain.Facts{
		Entities:  entities,
		Relations: relations,
	}, domain.Facts{}, nil
}

// matchWorkloadToPods is the core heuristic function that matches a workload with its pods.
// A pod is considered owned by a workload if:
// - The pod's name begins with the workload's full name
// - The pod is in the same namespace as the workload
// This works because Kubernetes workload controllers generate pod names by appending suffixes
// to the workload name (e.g., "nginx" deployment creates pods like "nginx-7d8f4c9b5-abc12").
func matchWorkloadToPods(workload domain.Entity, pods []domain.Pod) []domain.Relation {
	relations := make([]domain.Relation, 0)

	// Get workload name and namespace
	workloadName := workload.GetName()
	var workloadNamespace string
	if ns, ok := workload.(domain.Namespaced); ok {
		workloadNamespace = ns.GetNamespace()
	}

	// Match pods whose name starts with the workload name and are in the same namespace
	for _, pod := range pods {
		if pod.GetNamespace() == workloadNamespace && strings.HasPrefix(pod.GetName(), workloadName) {
			// Create Owns relation
			if ownable, ok := any(pod).(domain.Ownable); ok {
				relations = append(relations, domain.Owns{
					Owner:  workload,
					Object: ownable,
				})
			}
		}
	}

	return relations
}

// getWorkloads returns all Kubernetes workloads (Deployments, StatefulSets, DaemonSets, ReplicaSets, CronJobs)
// from the campaign's knowledge base.
func (c *Campaign) getWorkloads() []domain.Entity {
	workloads := make([]domain.Entity, 0)
	for _, e := range c.GetEntities() {
		switch e.(type) {
		case domain.Deployment, domain.StatefulSet, domain.DaemonSet, domain.ReplicaSet, domain.CronJob:
			workloads = append(workloads, e)
		}
	}
	return workloads
}

// analyzeWorkloadOwnership is called when a new workload is discovered.
// It matches the workload against all existing pods in the campaign.
func (c *Campaign) analyzeWorkloadOwnership(workload domain.Entity) (domain.Facts, error) {
	// Get all pods from the campaign
	allPods := c.GetPods()

	// Match the workload against all existing pods
	relations := matchWorkloadToPods(workload, allPods)

	return domain.Facts{
		Entities:  []domain.Entity{workload},
		Relations: relations,
	}, nil
}
