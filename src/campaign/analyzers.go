package campaign

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"regexp"
	"strconv"
	"strings"

	"github.com/Magier/Ran/domain"
	"github.com/google/uuid"
)

func (c Campaign) AnalyzeChanges(newFacts NewFacts, removedFacts RemovedFacts) (NewFacts, RemovedFacts, error) {
	entities := make(map[string]domain.Entity)
	relations := make([]domain.Relation, 0)
	identities := make(map[string]domain.Identity)
	assets := make([]domain.Asset, 0)
	// assets := make(map[string]domain.Asset)
	queue := append([]domain.Entity{}, newFacts.Entities...)

	// TODO: properly implement the integration of new facts, so they keep getting analyzed
	// but be aware of their nature, e.g. entity vs asset/identity

	// Index-based loop to avoid issues with appending while ranging
	for i := 0; i < len(queue); i++ {
		current := queue[i]
		switch e := current.(type) {
		case domain.Pod:
			// TODO: temporary hack: update pod before analyzing, to ensure all the information is available
			if prev, ok := c.GetEntityById(e.GetId()); ok {
				current = domain.UpdateEntity(e, prev)
			}

			entities[queue[i].GetId()] = current
			if e.NodeName != "" {
				node := domain.NewK8sNode(e.NodeName)
				if _, exists := entities[node.GetId()]; !exists {
					entities[node.GetId()] = node
				} else {
					entities[node.GetId()] = domain.UpdateEntity(entities[node.GetId()], node)
				}
				e.RunsOn = &node
				relations = append(relations, domain.RunsOn{Pod: e, Node: node})
				queue = append(queue, node)
			}

			if e.ServiceAccountName != "" {
				sa := domain.NewServiceAccount(e.ServiceAccountName, e.GetNamespace())
				if prevSA, exists := entities[sa.GetId()]; exists {
					sa = domain.UpdateEntity(prevSA, sa).(domain.ServiceAccount)
				}
				entities[sa.GetId()] = sa
				if e.AutomountServiceAccountToken.Bool() {
					saUsage := domain.Uses{
						SubjectId: e.GetId(),
						ObjectId:  sa.GetId(),
					}
					relations = append(relations, saUsage)
				} else {
					relations = append(relations, domain.Reference{
						Source: e.GetId(),
						Target: sa.GetId(),
					})
				}
			}

			resultingFacts, err := analyzeMountInfo(e)
			if err != nil {
				slog.Error("Failed to analyze SelfSubjectRulesReview", "error", err)
			} else {
				for _, entity := range resultingFacts.Entities {
					if existing, exists := entities[entity.GetId()]; exists {
						entity = domain.UpdateEntity(entity, existing)
					}
					entities[entity.GetId()] = entity
				}
				relations = append(relations, resultingFacts.Relations...)
			}

			// ensure to use the latest version of the pod
			e = entities[e.GetId()].(domain.Pod)
			newFacts1, err := analyzeHostPath(e)
			var _ = newFacts1 // to avoid unused variable warning
			if err != nil {
				slog.Warn("Failed to analyze host path for pod", "error", err, "pod", e.GetId())
			}

			// ensure to use the latest version of the pod
			e = entities[e.GetId()].(domain.Pod)
			hostRelations := analyzePodHostRelations(e)
			relations = append(relations, hostRelations...)
		case domain.ServiceAccountToken:
			resultingFacts, err := analyzeServiceAccountToken(e.Raw)
			if err != nil {
				slog.Error("Failed to analyze service account token", "error", err)
			} else {
				for _, entity := range resultingFacts.Entities {
					if e, exists := entities[entity.GetId()]; exists {
						entity = domain.UpdateEntity(entity, e)
					}
					entities[entity.GetId()] = entity
					queue = append(queue, entity)
				}

				for _, i := range resultingFacts.Identities {
					identities[i.GetId()] = i
				}

				assets = append(assets, resultingFacts.Assets...)
				relations = append(relations, resultingFacts.Relations...)
			}
		case domain.SelfSubjectRulesReview:
			resultingFacts, _, err := c.analyzeSelfSubjectRulesReview(e)
			if err != nil {
				slog.Error("Failed to analyze SelfSubjectRulesReview", "error", err)
			} else {
				for _, entity := range resultingFacts.Entities {
					if existing, exists := entities[entity.GetId()]; exists {
						entity = domain.UpdateEntity(entity, existing)
					}
					entities[entity.GetId()] = entity
				}
				relations = append(relations, resultingFacts.Relations...)
			}
		default:
			entities[e.GetId()] = e
			slog.Warn("Unknown entity type in processQueue", "entity", e)
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

	return NewFacts{
		Entities:   entitiesSlice,
		Relations:  relations,
		Assets:     assetsSlice,
		Identities: identitySlice,
	}, removedFacts, nil
}

func (c Campaign) analyzeSelfSubjectRulesReview(ssrr domain.SelfSubjectRulesReview) (NewFacts, RemovedFacts, error) {
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
					if len(rule.ResourceNames) == 0 {
						perm := domain.RBACPermission{
							Verb:         verb,
							ResourceType: resource,
							APIGroup:     apiGroup,
							Scope:        sa.GetNamespace(),
						}
						entitlements = append(entitlements, perm)
					} else {
						for _, resourceName := range rule.ResourceNames {
							perm := domain.RBACPermission{
								Verb:         verb,
								ResourceType: resource,
								ResourceName: resourceName,
								APIGroup:     apiGroup,
								Scope:        sa.GetNamespace(),
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
	return NewFacts{
		Entities:  entities,
		Relations: relations,
	}, RemovedFacts{}, nil
}

func inferBuiltinRBACRole(entitlements []domain.RBACPermission) string {
	return ""
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
func analyzeEnvironmentVariables(source domain.Entity, envVars map[string]string) (NewFacts, RemovedFacts, error) {
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

	return NewFacts{
		Entities:  entities,
		Relations: relations,
	}, RemovedFacts{}, nil
}

func analyzeServiceAccountToken(token string) (NewFacts, error) {
	parts := strings.SplitN(token, ".", 3)
	newFacts := NewFacts{}

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
	saToken.Raw = token
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

	nodeRunsPod := domain.Runs{
		Node: node,
		Pod:  pod,
	}
	pod.RunsOn = &node
	podRunsOnNode := domain.RunsOn{
		Pod:  pod,
		Node: node,
	}

	return NewFacts{
		Entities:  []domain.Entity{ns, sa, pod, node},
		Assets:    []domain.Asset{saToken},
		Relations: []domain.Relation{saUsage, nsContainsSa, nodeRunsPod, podRunsOnNode},
	}, nil

}

func analyzeFailedTTPExecution(ev domain.TTPExecuted) (NewFacts, RemovedFacts, error) {
	if len(ev.Results) == 0 {
		return NewFacts{}, RemovedFacts{}, fmt.Errorf("no results found for TTP execution %s, so nothing to analyze", ev.TTP.ID)
	}
	errMsg := ev.Results[0]
	if errMsg == "" && len(ev.Results) > 1 { // maybe the information is in stderr
		errMsg = ev.Results[1]
	}

	entities := make([]domain.Entity, 0)
	relations := make([]domain.Relation, 0)

	// the tool part of the procedure was not on the target system

	toolNotFoundMsgs := []string{
		fmt.Sprintf("%s: not found", ev.Procedure.GetTool()),
		"executable file not found in $PATH", // happened when using `k exec`
	}

	for _, toolNotFoundMsg := range toolNotFoundMsgs {
		if strings.Contains(errMsg, toolNotFoundMsg) || (len(ev.Results) > 1 && strings.Contains(ev.Results[1], toolNotFoundMsg)) {
			// "command terminated with exit code 127: 'sh: 1: kubectl: not found\n'"
			// "bash: wget: command not found"  on nginx pod
			if execSystem := ev.ExecutedOn; execSystem != nil {
				// if p, ok := target.(domain.Pod); ok
				execSystem.SetBinary(ev.Procedure.GetTool(), "")
				entities = append(entities, execSystem)
			}
		}
	}

	// TTP failed at the Kubernetes API server for various reasons (RBAC, admission control, etc.)
	// find out which type of error it was
	if strings.Contains(errMsg, "Error from server (Forbidden)") {
		// examples:
		// "command terminated with exit code 1: 'Error from server (Forbidden): pods is forbidden: User \"system:serviceaccount:dev:default\" cannot list resource \"pods\" in API group \"\" in the namespace \"dev\"\n'"
		if strings.Contains(errMsg, "is forbidden: User") { // heuristic: use this as hint for RBAC issue
			if sa, err := parseViolatingRBACIdentity(errMsg); err == nil {
				entities = append(entities, sa)
			} else {
				slog.Error(fmt.Sprintf("Failed to parse RBAC identity from error message: %s", err.Error()))
			}
		}

		// check for PSA failure
		podSecurityViolationPattern := regexp.MustCompile(`violates PodSecurity "([^"]+)": (.+)`)
		matches := podSecurityViolationPattern.FindStringSubmatch(errMsg)
		if len(matches) >= 3 {
			securityProfile := matches[1] // e.g., "baseline:latest"
			violationReason := matches[2] // e.g., "privileged ..."
			slog.Error(fmt.Sprintf("Pod creation failed due to security violation - Profile: %s, Reason: %s",
				securityProfile, violationReason))

			if target, ok := ev.Target.(domain.Namespaced); ok {
				ns := domain.Namespace{Name: target.GetNamespace(), EnforcedPSS: securityProfile, Kind: "Namespace"}
				entities = append(entities, ns)
			} else if nsName, ok := ev.Args["Namespace"]; ok {
				ns := domain.Namespace{Name: nsName, EnforcedPSS: securityProfile, Kind: "Namespace"}
				entities = append(entities, ns)
			} else {
				slog.Error("No namespace found in event target or args")
			}
		}
	}

	// TTP specific error: could not transfer tool to the target system
	if strings.Contains(errMsg, " is not writeable") {
		target := ev.Target
		if p, ok := target.(domain.Pod); ok {
			p.ReadOnlyRootFilesystem.Update(.3) // belief update that this flag is set
			entities = append(entities, p)
		} else {
			panic(fmt.Sprintf("TTP '%s' executed on non-pod target '%s'", ev.TTP.ID, target.GetId()))
		}
	}

	// example: violating PSA: "command terminated with exit code 1: 'Error from server (Forbidden): error when creating "STDIN": pods "bad-pod" is forbidden: violates PodSecurity "baseline:latest": hostPath volumes (volume "hostmount"), privileged (container "bad-pod" must not set securityContext.privileged=true)'"

	// Malformed procedure:
	// example:
	// "command terminated with exit code 1: 'error: error parsing STDIN: json: offset 138: invalid character '$' looking for beginning of value\n'"

	return NewFacts{
		Entities:  entities,
		Relations: relations,
	}, RemovedFacts{}, nil
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

func analyzeDeployPodResult(ev domain.TTPExecuted) (NewFacts, RemovedFacts, error) {
	relations := make([]domain.Relation, 0)

	newPod := domain.NewPod(
		ev.Args["Name"],
		ev.Args["Namespace"],
	)

	if saName, ok := ev.Args["ServiceAccount"]; ok {
		newPod.ServiceAccountName = saName
	}
	if val, ok := ev.Args["Privileged"]; ok {
		newPod.Privileged = domain.AsProbBool(strings.ToLower(val) == "true")
	}
	if val, ok := ev.Args["HostIPC"]; ok {
		newPod.HostIPC = domain.AsProbBool(strings.ToLower(val) == "true")
	}
	if val, ok := ev.Args["HostPID"]; ok {
		newPod.HostPID = domain.AsProbBool(strings.ToLower(val) == "true")
	}
	if val, ok := ev.Args["HostNetwork"]; ok {
		newPod.HostNetwork = domain.AsProbBool(strings.ToLower(val) == "true")
	}
	if nodeName, ok := ev.Args["NodeName"]; ok {
		newPod.NodeName = nodeName
	}

	if val, ok := ev.Args["HostPath"]; ok {
		mountPath := ev.Args["Mount"]
		newPod.VolumeMounts = []domain.Mount{{
			MountRoot:  val,
			MountPoint: mountPath,
			IsHostPath: true,
		}}
	}

	return NewFacts{
		Entities:  []domain.Entity{newPod},
		Relations: relations,
	}, RemovedFacts{}, nil
}

func analyzeDeployPodFailure(event domain.TTPExecuted) (NewFacts, RemovedFacts, error) {
	entities := make([]domain.Entity, 0)
	if len(event.Results) == 0 {
		slog.Error("No results found for deploy pod, so nothing to analyze :(")
	} else {
		res := event.Results[0]

		if strings.Contains(res, "already exists") {
			slog.Debug("Pod already exists, so nothing to analyze")
		} else if strings.Contains(res, "Error from server (Forbidden)") {
			// Check if the error message contains information about pod security violations
			podSecurityViolationPattern := regexp.MustCompile(`violates PodSecurity "([^"]+)": (.+)`)
			matches := podSecurityViolationPattern.FindStringSubmatch(res)
			if len(matches) >= 3 {
				securityProfile := matches[1] // e.g., "baseline:latest"
				violationReason := matches[2] // e.g., "privileged ..."
				slog.Error(fmt.Sprintf("Pod creation failed due to security violation - Profile: %s, Reason: %s",
					securityProfile, violationReason))

				if target, ok := event.Target.(domain.Namespaced); ok {
					ns := domain.Namespace{Name: target.GetNamespace(), EnforcedPSS: securityProfile, Kind: "Namespace"}
					entities = append(entities, ns)
				} else if ns, ok := event.Target.(domain.Namespace); ok {
					ns.EnforcedPSS = securityProfile
					entities = append(entities, ns)
				} else if nsName, ok := event.Args["Namespace"]; ok {
					ns := domain.Namespace{Name: nsName, EnforcedPSS: securityProfile, Kind: "Namespace"}
					entities = append(entities, ns)
				} else {
					slog.Error("No namespace found in event target or args")
				}
			}
		} else {
			slog.Error("Unknown error while deploying pod: " + res)
		}
	}

	return NewFacts{Entities: entities}, RemovedFacts{}, nil
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

func analyzeMountInfo(system domain.System) (NewFacts, error) {
	entities := make([]domain.Entity, 0)
	relations := make([]domain.Relation, 0)

	var node domain.K8sNode
	var foundNode bool
	var srcPod domain.Pod

	trackedHostPaths := map[string]bool{} // to avoid duplicates

	for _, mount := range system.GetMounts() {
		// kubelet-related paths are clearly part of the Node filesystem
		if strings.Contains(mount.MountPoint, "/var/lib/kubelet") {
			// one time logic to establish
			if !foundNode {
				foundNode = true
				node = domain.NewK8sNode("?")
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

			slog.Debug(fmt.Sprintf("Host path found: %s", mount.MountPoint))
		}
	}

	// TODO analyze the kubelet files on the node

	if foundNode {
		entities = append(entities, node)
	}
	if srcPod.Kind != "" { // Kind is automtically set when using a constructor like NewPod
		entities = append(entities, srcPod)
	}

	return NewFacts{
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

func analyzeHostPath(pod domain.Pod) (NewFacts, error) {
	entities := make([]domain.Entity, 0)
	relations := make([]domain.Relation, 0)

	hostPaths := extractHostPaths(pod.GetMounts()) // TODO: handle multiple results, if necessary

	if len(hostPaths) > 0 {
		pod.VolumeMounts = append(pod.VolumeMounts)

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

	return NewFacts{
		Entities:  entities,
		Relations: relations,
	}, nil
}

func analyzePodHostRelations(pod domain.Pod) []domain.Relation {
	rels := make([]domain.Relation, 0)

	for _, vm := range pod.VolumeMounts {
		if vm.IsHostPath {
			var node domain.K8sNode
			if pod.RunsOn == nil {
				node = domain.NewK8sNode(pod.NodeName)
			} else {
				node = *pod.RunsOn
			}
			rels = append(rels, domain.MountsHostPath{
				Pod:       pod,
				MountPath: vm.MountPoint,
				HostPath:  vm.MountRoot,
				Node:      node,
			})
		}
	}

	return rels
}

func analyzeToolSuccessfullyUsedInTTP(ev domain.TTPExecuted) (NewFacts, RemovedFacts, error) {
	// get the system on which the TTP was executed
	newFacts := NewFacts{}

	// TODO: support c2 channel with multiple segements
	if ev.ExecutedOn != nil {
		tool := ev.Procedure.GetTool()

		// add the binary only, if it was not yet known, because just a successful/failed
		// call provides no information of the exact path (which other info sources may do)
		if ev.ExecutedOn.HasBinary(tool).IsUnknown() {
			val := "❌"
			if ev.Success {
				val = tool
			}
			ev.ExecutedOn.SetBinary(tool, val)
		}
	}

	return newFacts, RemovedFacts{}, nil
}
