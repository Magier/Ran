package campaign

import (
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"os"
	"regexp"
	"strconv"
	"strings"
	"time"

	"github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
	k8s_types "github.com/Magier/Ran/k8sclient/types"
	"k8s.io/client-go/tools/clientcmd"
)

func parseK8sStatusResponse(str string) (k8s_types.K8sApiResponseStatus, error) {
	status, err := k8s.ParseStatus(str)
	return k8s_types.K8sApiResponseStatus{
		Code:    int(status.Code),
		Message: status.Message,
		Reason:  string(status.Reason),
		Status:  status.Status,
	}, err
}

func ParseEnvVarResult(_ map[string]string, results ...string) (map[string]string, error) {
	if len(results) == 0 {
		return nil, errors.New("No environment variables received!")
	}

	if len(results) > 1 {
		stderr := results[1]
		if stderr != "" {
			return nil, errors.New(stderr)
		}
	}

	stdout := results[0]
	vars := make(map[string]string)

	var sep = "\n"
	if strings.Contains(stdout, "\x00") {
		sep = "\x00"
	}

	for _, l := range strings.Split(stdout, sep) {
		k, v, ok := strings.Cut(l, "=")
		if ok {
			vars[k] = v
		}
	}

	return vars, nil
}

func parseRawServiceAccountToken(args ...string) (domain.ServiceAccountToken, error) {
	// func HandleSaTokenRead(source domain.Entity, args ...string) (domain.Event, error) {
	if len(args) == 0 {
		return domain.ServiceAccountToken{}, fmt.Errorf("No SA token provided as argument")
	}

	var token string = args[0]
	if len(token) == 0 {
		return domain.ServiceAccountToken{}, fmt.Errorf("Empty SA token can't be decoded")
	}
	if len(args) > 1 && args[1] != "" {
		slog.Warn(fmt.Sprintf("Sa Token Read expects exactly 1 argument - received %d", len(args)))
	}

	// clean it up if necessary
	if strings.Contains(args[0], "\n") {
		for _, part := range strings.Split(args[0], "\n") {
			// naive heuristic to find the token: they are Base64 encoded which starts with "{"
			// and has 2 '.' to separate the parts of the JWT token
			if strings.Contains(part, "ey") && strings.Contains(part, ".") {
				token = part
				break
			}
		}
	}

	return domain.ServiceAccountToken{Raw: token}, nil

}

func parseSelfSubjectRulesReview(args ...string) (domain.SelfSubjectRulesReview, error) {
	// try parse JSON
	var ssrr domain.SelfSubjectRulesReview
	if len(args) == 0 {
		return ssrr, fmt.Errorf("No data")
	}
	var result k8s_types.SelfSubjectRulesReview
	var err error

	data := args[0]

	// Check if jsonData is valid JSON, otherwise try to parse as pretty-printed table
	if json.Valid([]byte(data)) {
		err = json.Unmarshal([]byte(data), &result)
		if err != nil {
			return ssrr, err
		}
	} else {
		slog.Warn("Input is not valid JSON, attempting to parse as pretty-printed SelfSubjectRulesReview")
		result, err = parsePrettySelfSubjectRulesReview(data)
		if err != nil {
			return ssrr, fmt.Errorf("Failed to parse pretty-printed SelfSubjectRulesReview: %w", err)
		}
	}

	if result.Code >= 400 {
		return ssrr, fmt.Errorf("SelfSubjectRulesReview failed with code %d: %s", result.Code, result.Message)
	}

	if result.Status.Incomplete {
		slog.Warn("Results from SelfSubjectRulesReview are incomplete!")
	}

	return domain.SelfSubjectRulesReview{
		ResourceRules:    result.Status.ResourceRules,
		NonResourceRules: result.Status.NonResourceRules,
	}, nil
}

func parsePrettySelfSubjectRulesReview(data string) (k8s_types.SelfSubjectRulesReview, error) {
	resRules := []k8s_types.ResourceRule{}
	nonResRules := []k8s_types.NonResourceRule{}
	lines := strings.Split(data, "\n")
	var row [][]string

	// Skip the header and parse remaining lines
	for _, line := range lines[1:] {
		fields := strings.SplitN(line, "[", 4)
		// clean every cell by dropping the closing ']' and trimming whitespace
		row = make([][]string, 4)
		for i := range fields {
			f := strings.TrimSuffix(strings.TrimSpace(fields[i]), "]")
			if f == "" {
				row[i] = []string{}
			} else {
				row[i] = strings.Split(f, " ")
			}
		}

		// empty "Resources" column means it's a NonResourceRule
		if len(row[0]) == 0 {
			nonResRules = append(nonResRules, k8s_types.NonResourceRule{
				NonResourceURLs: row[0],
				Verbs:           row[3],
			})
		} else {
			resRules = append(resRules, k8s_types.ResourceRule{
				APIGroups:     row[0], // assuming default API group
				Resources:     row[0],
				ResourceNames: row[2],
				Verbs:         row[3],
			})
		}
	}
	// Note: full JSON resopnse is impossible to reproduce, as the grouping is not based on the data itself
	return k8s_types.SelfSubjectRulesReview{
		Status: k8s_types.SubjectRulesReviewStatus{
			ResourceRules:    resRules,
			NonResourceRules: nonResRules,
		},
	}, nil
}

func parsePod(args map[string]string, results ...string) (domain.Pod, error) {
	var cfg domain.PodConfig
	var nsName, podName string

	numArgs := len(args)
	if numArgs < 2 {
		return domain.Pod{}, fmt.Errorf("Not enough arguments provided: expected at least 2, got %d", numArgs)
	}
	podName = args["Name"]
	nsName = args["Namespace"]

	if numArgs >= 3 {
		cfg.NodeName = args["NodeName"]
		cfg.ServiceAccount = args["ServiceAccount"]

		isPrivileged, _ := strconv.ParseBool(args["Privileged"])
		cfg.Privileged = isPrivileged

		hostIPC, _ := strconv.ParseBool(args["HostIPC"])
		cfg.HostIPC = hostIPC

		hostNetwork, _ := strconv.ParseBool(args["HostNetwork"])
		cfg.HostNetwork = hostNetwork

		hostPID, _ := strconv.ParseBool(args["HostPID"])
		cfg.HostPID = hostPID

		priv, _ := strconv.ParseBool(args["Privileged"])
		cfg.Privileged = priv

		if hostPath, ok := args["HostPath"]; ok {
			mountPath := args["Mount"]
			cfg.HostMounts = []domain.Mount{
				{MountPoint: mountPath, MountRoot: hostPath, IsHostPath: true, ReadOnly: false, Flags: []string{"rw"}},
			}
		}
	} else if len(results) >= 3 {
		// TODO: marshal the podConfig
		err := json.Unmarshal([]byte(results[2]), &cfg)
		if err != nil {
			return domain.Pod{}, fmt.Errorf("Failed to unmarshal PodConfig JSON: %w", err)
		}
	}

	p := domain.NewPod(podName, nsName)

	p.HostIPC = domain.AsProbBool(cfg.HostIPC)
	p.HostPID = domain.AsProbBool(cfg.HostPID)
	p.HostNetwork = domain.AsProbBool(cfg.HostNetwork)
	p.Privileged = domain.AsProbBool(cfg.Privileged)
	p.ServiceAccountName = cfg.ServiceAccount
	p.NodeName = cfg.NodeName
	p.VolumeMounts = cfg.HostMounts

	// return domain.NewPodDeployed{
	// 	Pod:       p,
	// 	Namespace: ns,
	// }, nil
	return p, nil
}

func HandleNewCronJob(ev domain.TTPExecuted, source domain.Entity, ttpArgs map[string]string, results ...string) (domain.Event, error) {
	numArgs := len(results)
	if numArgs == 0 {
		return nil, fmt.Errorf("No data")
	}

	podName := results[0]
	nsName := results[1]
	if nsName == "" {
		if src, ok := source.(domain.K8sEntity); ok {
			nsName = src.GetNamespace()
		} else {
			return nil, fmt.Errorf("source does not have a namespace")
		}
	}
	ns := domain.NewNamespace(nsName)
	p := domain.NewPod(podName, nsName)

	if len(results) >= 3 {
		// TODO: marshal the podConfig
		var cfg domain.PodConfig
		err := json.Unmarshal([]byte(results[2]), &cfg)
		if err != nil {
			return nil, fmt.Errorf("Failed to unmarshal PodConfig JSON: %w", err)
		}
		// cfgJson := args[2].(domain.PodConfig)

		p.HostIPC = domain.AsProbBool(cfg.HostIPC)
		p.HostPID = domain.AsProbBool(cfg.HostPID)
		p.HostNetwork = domain.AsProbBool(cfg.HostNetwork)
		p.Privileged = domain.AsProbBool(cfg.Privileged)
	}

	// TODO: this should also add the new CronJob to the knowledge base, which owns this pod

	slog.Error(fmt.Sprintf("Creating new pod %s in namespace %s is not yet properly implemented! FIX NEEDED!", p.Name, ns.Name))
	return domain.NewPodDeployed{
		Pod:       p,
		Namespace: ns,
	}, nil
}

func ParseSecretList(jsonStr string) ([]domain.K8sSecret, error) {
	secretList, err := k8s.ParseSecretList(jsonStr)
	if err != nil {
		return nil, fmt.Errorf("Failed to parse SecretList: %w", err)
	}

	secrets := make([]domain.K8sSecret, 0, len(secretList.Items))
	for _, item := range secretList.Items {
		secrets = append(secrets, domain.NewSecretFromK8sSpec(item))
	}

	return secrets, nil
}

func ParseConfigMapList(jsonStr string) ([]domain.ConfigMap, error) {
	configMapList, err := k8s.ParseConfigMapList(jsonStr)
	if err != nil {
		return nil, fmt.Errorf("Failed to parse ConfigMapList: %w", err)
	}

	configMaps := make([]domain.ConfigMap, 0, len(configMapList.Items))
	for _, item := range configMapList.Items {
		configMaps = append(configMaps, domain.NewConfigMapFromK8sSpec(item))
	}

	return configMaps, nil
}

func (c *Campaign) ParseEffect(effect string, source domain.Entity, args map[string]string, results ...string) (factsUpdate, error) {
	if len(results) == 0 {
		return factsUpdate{}, fmt.Errorf("Can't parse effect %s because there are no results", effect)
	}

	if strings.Contains(results[0], "already exists") {
		slog.Info(fmt.Sprintf("Parsing Effect: entity '%s' already exists", effect))
	}

	isRemoveEffect := strings.HasPrefix(effect, "delete")
	effect = strings.TrimPrefix(effect, "delete ")
	isCreateEffect := strings.HasPrefix(effect, "create")
	if isCreateEffect {
		effect = strings.ToLower(strings.TrimPrefix(effect, "create "))
	}

	res := results[0]
	entities := []domain.Entity{}
	relations := []domain.Relation{}

	// ensure it's not a failure response from the K8s API server
	if strings.HasPrefix(effect, "k8s.") {
		status, err := parseK8sStatusResponse(results[0])
		// in this case having an error is good -> it's not an unexpected StatusResponse
		if err == nil && status.Code >= 400 {
			return factsUpdate{}, k8s_types.K8sAPIResponseError{Status: status}
		} else if strings.Trim(res, " ") == "error: cannot exec into a container in a completed pod; current phase is Succeeded" {
			p := source.(domain.Pod)
			p.IsRunning = false
			entities = append(entities, p)
			facts := domain.Facts{Entities: entities, Relations: relations}
			return factsUpdate{New: facts}, fmt.Errorf("Pod is in Succeeded phase, can't exec into it")
		}
	}

	var facts domain.Facts
	if strings.HasPrefix(effect, "k8s.") {
		f, err := parseK8sEffect(effect, source, args, results)
		if err != nil {
			return factsUpdate{}, fmt.Errorf("Failed to parse K8s effect: %w", err)
		}
		facts = f
	} else {
		switch strings.ToLower(effect) {
		case "sys.ip":
			if sys, ok := source.(domain.System); ok {
				ips := []net.IPAddr{}
				res := results[0]
				for _, ip := range strings.Split(res, " ") {
					parsedIP := net.ParseIP(ip)
					if parsedIP == nil {
						slog.Error("Failed to parse IP")
						break
					}
					ips = append(ips, net.IPAddr{IP: parsedIP})
				}
				sys.SetIPs(ips)
				entities = append(entities, sys)
			}
		case "sys.hasbinary":
			// TODO: merge with alternative, more generic parser for `has-binary` effect in `default` branch, after exploration
			if sys, ok := source.(domain.Pod); ok {
				binaryName := ""
				dstPath := args["DST_PATH"]

				if dstPath != "" {
					parts := strings.Split(dstPath, "/")
					binaryName = parts[len(parts)-1]
				} else if strings.HasPrefix(results[0], "file: ") {
					// get the information from the TTP result (explicit echo at the end of the TTP)
					// Note: tight coupling with the TTP implementation -> brittle
					dstPath = strings.TrimPrefix(results[0], "file: ")
				} else {
					// fallback try to extract the binary name from the source name
					// however: knowing the location of the binary on the system is not always possible
					dstPath = ""
					// TODO: get the path from the SRC_PATH?
					slog.Warn("No DST_PATH provided, and extraction from SRC_PATh is not yet implemented!")
				}
				sys.SetBinary(binaryName, dstPath)
				entities = append(entities, sys)
			} else {
				slog.Warn("The source of the hasBinary effect is not a Pod!")
			}
		case "rawserviceaccounttoken":
			saToken, err := parseRawServiceAccountToken(results...)
			if err != nil {
				slog.Error(fmt.Sprintf("Failed to parse raw service account token: %v", err))
			} else {
				entities = append(entities, saToken)
			}
		case "sys.envvar":
			envVars, err := ParseEnvVarResult(args, res)
			if err != nil {
				slog.Error(fmt.Sprintf("Could not parse environment variable: %v", err))
			}

			if sys, ok := source.(domain.System); ok {
				sys.SetEnvironmentVariables(envVars)
				newFacts, _, err := analyzeEnvironmentVariables(source, envVars)
				if err != nil {
					slog.Error(fmt.Sprintf("Failure analyzing environment variables %v", err))
				} else {
					entities = append(entities, newFacts.Entities...)
					relations = append(relations, newFacts.Relations...)
				}
			} else {
				panic("The source should implement the System interface!")
			}
		case "linux.mounts":
			if pod, ok := source.(domain.Pod); ok {
				mounts, err := parseLinuxMounts(res)
				if err != nil {
					slog.Error(fmt.Sprintf("Failed to parse Linux mounts: %v", err))
				} else {
					pod.Mounts = append(pod.Mounts, mounts...)
					entities = append(entities, pod)
				}
			}
		case "sys.processes":
			if sys, ok := source.(domain.System); ok {
				processes, err := parseLinuxProcesses(res)
				if err != nil {
					slog.Error(fmt.Sprintf("Failed to parse processes: %v", err))
				} else {
					sys.SetProcesses(processes)
					entities = append(entities, sys)
				}
			} else {
				slog.Warn("The source of the processes effect is not a System!")
			}
		case "sys.userid":
			if sys, ok := source.(domain.System); ok {
				uid, username, err := parseLinuxIDResult(res)
				if err != nil {
					slog.Error(fmt.Sprintf("Failed to parse user ID: %v", err))
				} else {
					sys.SetUserID(uid)
					sys.SetUserName(username)
					entities = append(entities, sys)
				}
			} else {
				slog.Warn("The source of the user ID effect is not a System!")
			}
		case "sys.files":
			fsEntries, err := parseFiles(results[0])
			srcDir := args["DIR"]
			if err != nil {
				slog.Error(fmt.Sprintf("Failed to parse files: %v", err))
			} else {
				if sys, ok := source.(domain.System); ok {
					files := []string{}
					for _, entry := range fsEntries {
						fullPath := fmt.Sprintf("%s/%s", srcDir, entry.Name)
						if entry.IsExec {
							// also explicitely track all binaries
							sys.SetBinary(entry.Name, fullPath)
						}
						files = append(files, fullPath)
					}
					sys.AddFiles(files)
					entities = append(entities, sys)
				}
			}
		case "file:kubeconfig":
			if len(results[0]) > 0 {
				config, err := loadKubeConfigFromString(results[0])
				if err != nil {
					slog.Error(fmt.Sprintf("Failed to load kubeconfig: %v", err))
				} else {
					// Extract AuthInfo (user) from kubeconfig
					rawConfig, err := config.RawConfig()
					if err != nil {
						slog.Error(fmt.Sprintf("Failed to get raw kubeconfig: %v", err))
					} else {
						currentContext := rawConfig.CurrentContext
						ctx := rawConfig.Contexts[currentContext]
						if ctx != nil {
							cluster := domain.NewCluster(ctx.Cluster, "")
							slog.Warn(fmt.Sprintf("Kubeconfig effect: using cluster '%s' has not implementing parsing of its address", ctx.Cluster))
							// cluster := domain.NewCluster(ctx.Cluster, rawConfig.Clusters[ctx.Cluster].Cluster.Server)
							entities = append(entities, cluster)

							authInfo := ctx.AuthInfo
							user := domain.User{
								Name:     authInfo,
								IsAdmin:  true, // TODO: infer if the user is admin based on the kubeconfig
								Kind:     "User",
								CertData: rawConfig.AuthInfos[authInfo].ClientCertificateData,
								KeyData:  rawConfig.AuthInfos[authInfo].ClientKeyData,
							}
							entities = append(entities, user)

							relations = append(relations, domain.Contains{
								Container: cluster,
								Object:    user,
							})
						}
					}
				}
			}
		default:
			relationName, relationArgs, err := parseRelationEffect(effect)
			var _ = relationName
			var _ = relationArgs
			// resultingEntity, err := parseHasBinaryEffect(source, effect, args, results...)
			if err != nil {
				slog.Error(fmt.Sprintf("Failed to parse relation effect: %v", err))
			} else {
				if strings.HasPrefix(effect, "sys.has-binary") {

					resultingEntity, err := parseHasBinaryEffect(source, effect, args, results...)
					if err != nil {
						slog.Error(fmt.Sprintf("Failed to parse has-binary effect: %v", err))
					} else {
						entities = append(entities, resultingEntity)
					}
				} else if strings.HasPrefix(effect, "sys.hasfile") {
					resultingEntity, err := parseHasBinaryEffect(source, effect, args, results...)
					if err != nil {
						slog.Error(fmt.Sprintf("Failed to parse has-binary effect: %v", err))
					} else {
						entities = append(entities, resultingEntity)
					}
				}
			}
		}

		facts = domain.Facts{Entities: entities, Relations: relations}
	}

	if isRemoveEffect {
		return factsUpdate{Removed: facts}, nil
	}
	return factsUpdate{New: facts}, nil
}

func parseRelationEffect(relation string) (string, []string, error) {
	// Example "k8s.can-exec(C2, Pod)", where Pod is the class and refers to all pods, or sometimes it's direct instances
	// like k8s.hasFile(system, "/etc/kubernetes/admin.conf")

	// match the relation before the parenthesis and the varidic parameters within the parenthesis
	re := regexp.MustCompile(`^(.*?)\(\s*(.*?)\s*\)$`)
	match := re.FindStringSubmatch(relation)
	if len(match) > 2 {
		relation = strings.ToLower(match[1])
		variables := strings.Split(match[2], ",")
		if match[2] == "" {
			variables = []string{}
		}
		for i := range variables {
			variables[i] = strings.TrimSpace(variables[i])
		}
		return strings.TrimSpace(relation), variables, nil
	}
	return "", nil, fmt.Errorf("Invalid relation format: %s", relation)
}

func parseK8sEffect(effect string, source domain.Entity, args map[string]string, results []string) (domain.Facts, error) {
	entities := []domain.Entity{}
	relations := []domain.Relation{}
	res := results[0]

	switch strings.ToLower(effect) {
	case "k8s.selfsubjectrulesreview":
		ssrr, err := parseSelfSubjectRulesReview(results...)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse SelfSubjectRulesReview: %v", err))
		} else {
			sa, ok := source.(domain.ServiceAccount)
			if !ok {
				slog.Warn("the source of the SubjectReviewResult is not a valid ServiceAccount!")
			} else {
				ssrr.ServiceAccount = sa
				ssrr.TokenName = sa.GetName()
			}
			// TODO: temporary workaround to treat SelfSubjectRulesReview as an entity, so it's processed in the analyzer
			entities = append(entities, ssrr)
		}
	case "k8s.podlist":
		list, err := k8s.ParsePodList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse PodList: %v", err))
		} else {
			for _, res := range list.Items {
				pod := domain.NewPodFromK8sSpec(res)
				entities = append(entities, pod)
			}
		}
	case "k8s.deploymentlist":
		list, err := k8s.ParseDeploymentList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse DeploymentList: %v", err))
		} else {
			for _, res := range list.Items {
				entities = append(entities, domain.NewDeploymentFromK8sSpec(res))
			}
		}
	case "k8s.serviceaccountlist":
		list, err := k8s.ParseServiceAccountList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse ServiceAccountList: %v", err))
		} else {
			for _, res := range list.Items {
				entities = append(entities, domain.NewServiceAccountFromK8sSpec(res))
			}
		}
	case "k8s.serviceaccount":
		name := args["Name"]
		ns := args["Namespace"]
		sa := domain.NewServiceAccount(name, ns)
		entities = append(entities, sa)
	case "k8s.pod":
		p, err := parsePod(args, results...)
		if err != nil {
			slog.Error(fmt.Sprintf("Failed to parse Pod: %v", err))
		} else {
			entities = append(entities, p)
		}
	case "k8s.deployment":
		name := args["Name"]
		ns := args["Namespace"]
		pod := domain.NewDeployment(name, ns)
		entities = append(entities, pod)
	case "k8s.secretlist":
		secrets, err := ParseSecretList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse SecretList: %v", err))
		} else {
			for _, secret := range secrets {
				entities = append(entities, secret)
			}
		}
	case "k8s.configmaplist":
		configMaps, err := ParseConfigMapList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse ConfigMapList: %v", err))
		} else {
			for _, configMap := range configMaps {
				entities = append(entities, configMap)
			}
		}
	case "k8s.nodelist":
		nodeList, err := k8s.ParseNodeList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Failed to parse NodeList: %v", err))
		} else {
			for _, node := range nodeList.Items {
				entities = append(entities, domain.NewK8sNodeFromK8sSpec(node))
			}
		}
	case "k8s.role":
		role, err := parseRBACRole(args, results...)
		if err != nil {
			slog.Error(fmt.Sprintf("Failed to parse Role: %v", err))
		} else {
			entities = append(entities, role)
		}
	case "k8s.rolebinding":
		binding, err := parseRBACRoleBinding(args, results...)
		if err != nil {
			slog.Error(fmt.Sprintf("Failed to parse Role: %v", err))
		} else {
			entities = append(entities, binding)
		}
	case "k8s.cronjob":
		cronJob, err := parseK8sCronJob(args, results...)
		if err != nil {
			slog.Error(fmt.Sprintf("Failed to parse CronJob: %v", err))
		} else {
			entities = append(entities, cronJob)
		}
	default:
		if strings.Contains(effect, "(") && strings.Contains(effect, ")") {
			relationName, relationArgs, err := parseRelationEffect(effect)
			if err != nil {
				slog.Error(fmt.Sprintf("Failed to parse relation effect: %v", err))
			} else {
				if strings.HasPrefix(effect, "k8s.can-exec") {
					if len(relationArgs) != 2 {
						return domain.Facts{}, fmt.Errorf("k8s.can-exec effect expects exactly 2 arguments: C2 and PodName")
					}
					c2ID := relationArgs[0]
					targetID := relationArgs[1]

					// TODO: handle variables in effects more generically
					if targetID == "${TARGET}" {
						targetID = source.GetId()
					}

					switch relationName {
					case "k8s.can-exec":
						relations = append(relations, domain.CanAccess{
							SourceId:    c2ID,
							TargetId:    targetID,
							AccessLevel: domain.UserExec,
							PodsExec:    true,
						})
					}
				}
			}
		} else if strings.Contains(effect, ".IsRunning") {
			// expected format: "k8s.pod.isRunning=<boolvalue>"
			parts := strings.SplitN(effect, "=", 2)
			if len(parts) != 2 {
				slog.Warn(fmt.Sprintf("isRunning effect missing value: %s", effect))
			} else {
				val := strings.TrimSpace(parts[1])
				isRunning, err := strconv.ParseBool(val)
				if err != nil {
					slog.Error(fmt.Sprintf("Failed to parse isRunning value '%s': %v", val, err))
				} else {
					var pod domain.Pod
					var ok bool
					if pod, ok = source.(domain.Pod); !ok {
						pod = domain.NewPod(args["Name"], args["Namespace"])
					}
					pod.IsRunning = isRunning
					entities = append(entities, pod)
					// } else if len(relationArgs) >= 2 {
					// 	// fallback: try to construct a Pod from relation args (e.g. k8s.pod.isRunning(C2, podName) style)
					// 	pod := domain.NewPod(relationArgs[1], "")
					// 	pod.IsRunning = isRunning
					// 	entities = append(entities, pod)
				}
			}
		}
	}
	return domain.Facts{Entities: entities, Relations: relations}, nil
}

func parseHasBinaryEffect(source domain.Entity, effect string, args map[string]string, results ...string) (domain.Entity, error) {
	// Extract the binary name from the effect string, e.g. "target.has-binary(${BINARY_NAME})"
	// Match parameter within the parenthesis of `has-binary()`
	re := regexp.MustCompile(`\(\s*\s*(.*?)\s*\)`)

	match := re.FindStringSubmatch(effect)
	if len(match) > 1 {
		paramName := strings.ToUpper(match[1])
		if sys, ok := source.(domain.System); ok {
			if IsTemplateVariable(paramName) {
				paramName = strings.TrimPrefix(paramName, "${")
				paramName = strings.TrimSuffix(paramName, "}")
			}

			if binPath, ok := args[paramName]; ok {
				var binaryName = binPath
				if strings.Contains(binPath, "/") {
					parts := strings.Split(binPath, "/")
					binaryName = parts[len(parts)-1]
				}
				sys.SetBinary(binaryName, binPath) // same name implies it's a globally available binary
			} else {
				slog.Warn(fmt.Sprintf("Effect '%s' expects a parameter '%s' but it was not provided", effect, paramName))
			}
			return sys, nil
		} else {
			slog.Warn("The source of the has-binary effect is not a Pod!")
		}
	} else {
		slog.Warn("No parameter found in the has-binary effect, expected format: target.has-binary(${BINARY_NAME})")
	}
	return nil, fmt.Errorf("Invalid has-binary effect: %s", effect)
}

func loadKubeConfigFromString(configStr string) (clientcmd.ClientConfig, error) {
	config, err := clientcmd.NewClientConfigFromBytes([]byte(configStr))
	if err != nil {
		return nil, fmt.Errorf("failed to load kubeconfig: %w", err)
	}

	return config, nil
}

// Source: https://pkg.go.dev/github.com/moby/sys/mountinfo#Info for a struct that properly parses the mountinfo
type MountInfo struct {
	// ID is a unique identifier of the mount (may be reused after umount).
	ID int

	// Parent is the ID of the parent mount (or of self for the root
	// of this mount namespace's mount tree).
	Parent int

	// Major and Minor are the major and the minor components of the Dev
	// field of unix.Stat_t structure returned by unix.*Stat calls for
	// files on this filesystem.
	Major, Minor int

	// Root is the pathname of the directory in the filesystem which forms
	// the root of this mount.
	Root string

	// Mountpoint is the pathname of the mount point relative to the
	// process's root directory.
	Mountpoint string

	// Options is a comma-separated list of mount options.
	Options string

	// Optional are zero or more fields of the form "tag[:value]",
	// separated by a space.  Currently, the possible optional fields are
	// "shared", "master", "propagate_from", and "unbindable". For more
	// information, see mount_namespaces(7) Linux man page.
	Optional string

	// FSType is the filesystem type in the form "type[.subtype]".
	FSType string

	// Source is filesystem-specific information, or "none".
	Source string

	// VFSOptions is a comma-separated list of superblock options.
	VFSOptions string
}

type MountEntryParserFn func(string) (domain.Mount, error)

func getMountEntryParser(entry string) MountEntryParserFn {
	fields := strings.Fields(entry)
	// Check if the data contains the expected fields for mountinfo
	if len(fields) == 10 {
		// first 2 are ints
		// then major:minor of the Dev field of unix.Stat_t structure
		// then Root, Mountpoint, Options, Optional, FSType, Source, VFSOptions
		// see https://pkg.go.dev/github.com/moby/sys/mountinfo#Info for a struct that properly parses the mountinfo
		return parseMountInfoEntry
	}

	// example: overlay on /host type overlay (rw,relatime,...)
	// match the `on` and `type` static keywords
	if len(fields) >= 6 && fields[1] == "on" && fields[3] == "type" {
		return parseMountCommandEntry
	}

	// Check if the data contains the expected fields for mounts
	if len(fields) == 6 {
		// TODO: refine this check e.g. by checking the last 2 fields are ints
		return parseProcMountEntry
	}

	return nil
}

func parseMountInfoEntry(line string) (domain.Mount, error) {
	// first 2 are ints: ID and ParentID
	// then major:minor of the Dev field of unix.Stat_t structure
	// then Root, Mountpoint, Options, Optional, FSType, Source, VFSOptions
	// see https://pkg.go.dev/github.com/moby/sys/mountinfo#Info for a struct that properly parses the mountinfo

	var mount domain.Mount
	fields := strings.Fields(line)
	if len(fields) < 10 {
		return mount, fmt.Errorf("Invalid mountinfo entry: %s", line)
	}

	// majorMinor := strings.Split(fields[2], ":")
	// if len(majorMinor) != 2 {
	// 	return mount, fmt.Errorf("Invalid Major:Minor in mountinfo entry: %s", line)
	// }

	flags := strings.Split(fields[5], ",")

	isReadOnly := false
	for _, flag := range flags {
		if flag == "ro" || flag == "readonly" {
			isReadOnly = true
			break
		}
	}

	id, err := strconv.Atoi(fields[0])
	if err != nil {
		return mount, fmt.Errorf("Invalid ID in mountinfo entry: %s", line)
	}
	parentID, err := strconv.Atoi(fields[1])
	if err != nil {
		return mount, fmt.Errorf("Invalid ParentID in mountinfo entry: %s", line)
	}

	return domain.Mount{
		ID:         id,
		ParentID:   parentID,
		MountRoot:  fields[3],
		MountPoint: fields[4],
		Type:       fields[7],
		ReadOnly:   isReadOnly,
		Flags:      flags,

		// ID:         id,
		// Parent:     parent,
		// Major:      major,
		// Minor:      minor,
		// Root:       fields[3],
		// Mountpoint: fields[4],
		// Options:    fields[5],
		// Optional:   fields[6],
		// FSType:     fields[7],
		// Source:     fields[8],
		// VFSOptions: fields[9],
	}, nil
}

func parseProcMountEntry(line string) (domain.Mount, error) {
	// /dev/sda1 / ext4 rw,relatime,data=ordered 0 0
	// fields: [device mountpoint fstype options dump pass]
	return domain.Mount{}, fmt.Errorf("Parsing proc mount entry is not implemented yet: %s", line)
}

func parseMountCommandEntry(line string) (domain.Mount, error) {
	// /dev/sda1 / ext4 rw,relatime,data=ordered 0 0
	fields := strings.Fields(line)
	device := fields[0]    // e.g. /dev/sda1
	_ = fields[1]          // "on"
	mountPath := fields[2] // e.g. /mnt/host
	fsType := fields[4]    // e.g. overlay
	// Trim surrounding parentheses from options field
	opts := strings.Trim(fields[5], "()")
	options := strings.Split(opts, ",") // e.g. rw,relatime,...

	readOnly := false
	for _, opt := range options {
		if opt == "ro" || opt == "readonly" {
			readOnly = true
			break
		}
	}

	// fields: [device mountpoint fstype options dump pass]
	return domain.Mount{
		MountRoot:  device,
		MountPoint: mountPath,
		Type:       fsType,
		ReadOnly:   readOnly,
		Flags:      options,
	}, nil
}

func parseLinuxMounts(data string) ([]domain.Mount, error) {
	lines := strings.Split(data, "\n")

	// format := inferLinuxMountFormat(data)
	parserFn := getMountEntryParser(lines[0])
	if parserFn == nil {
		return nil, fmt.Errorf("Could not infer mount entry parser from the first line: %s", lines[0])
	}

	mounts := make([]domain.Mount, 0, len(lines))

	for _, line := range lines {
		if line == "" {
			continue
		}
		mount, err := parserFn(line)
		if err != nil {
			return nil, fmt.Errorf("Failed to parse mount entry '%s': %w", line, err)
		} else {
			mounts = append(mounts, mount)
		}
	}

	return mounts, nil
}

func parseLinuxProcesses(data string) ([]domain.Process, error) {
	lines := strings.Split(data, "\n")

	if len(lines) < 2 {
		return nil, fmt.Errorf("No process entries found in the data")
	}

	procs := make([]domain.Process, 0, len(lines))

	for i := 1; i < len(lines); i++ {
		ps, err := parseProcessStatus(lines[i])
		if err != nil {
			return nil, fmt.Errorf("Failed to parse process entry '%s': %w", lines[i], err)
		} else {
			procs = append(procs, ps)
		}
	}

	return procs, nil
}

func parseProcessStatus(line string) (domain.Process, error) {
	// Example line: "root         649  0.2  0.3 179420  7060 pts/0    Ss   20:28   0:00 /usr/bin/bash"
	fields := strings.Fields(line)
	if len(fields) < 5 {
		return domain.Process{}, fmt.Errorf("Invalid process status line: %s", line)
	}
	cmd := strings.Join(fields[7:], " ") // Join the rest as command

	pid, err := strconv.Atoi(fields[1])
	if err != nil {
		return domain.Process{}, fmt.Errorf("Invalid PID in process status line: %s", line)
	}

	ppid, err := strconv.Atoi(fields[2])
	if err != nil {
		return domain.Process{}, fmt.Errorf("Invalid PPID in process status line: %s", line)
	}

	cpu, err := strconv.Atoi(fields[3])
	if err != nil {
		return domain.Process{}, fmt.Errorf("Invalid CPU value in process status line: %s", line)
	}

	return domain.Process{
		UID:       fields[0],
		PID:       pid,
		ParentPID: ppid,
		CPU:       cpu,
		StartTime: fields[4],
		TTY:       fields[5],
		Time:      fields[6],
		Cmd:       cmd,
	}, nil
}

func parseLinuxIDResult(line string) (int, string, error) {
	// example: uid=0(root) gid=0(root) groups=0(root)
	fields := strings.Fields(line)
	for _, field := range fields {
		if strings.HasPrefix(field, "uid=") {
			uidStr := strings.Split(field, "=")[1]
			username := ""
			if strings.Contains(uidStr, "(") {
				parts := strings.Split(uidStr, "(")
				uidStr = parts[0]
				username = strings.TrimSuffix(parts[1], ")")
			}
			uid, err := strconv.Atoi(uidStr)
			if err != nil {
				return 0, "", fmt.Errorf("Invalid UID in line: %s", line)
			}
			return uid, username, nil
		}
	}

	return 0, "", fmt.Errorf("No UID found in line: %s", line)
}

func parseRBACRole(args map[string]string, results ...string) (domain.Role, error) {
	if strings.Contains(results[0], "Error from server (Forbidden)") {
		// "command terminated with exit code 1: 'Error from server (Forbidden): roles.rbac.authorization.k8s.io \"nsadmin\" is forbidden: user \"system:serviceaccount:dev:developer\" (groups=[\"system:serviceaccounts\" \"system:serviceaccounts:dev\" \"system:authenticated\"]) is attempting to grant RBAC permissions not currently held:\n{APIGroups:[\"\"], Resources:[\"*\"], Verbs:[\"*\"]}\n'"
		if strings.Contains(results[0], "attempting to grant RBAC permissions not currently held") {
			return domain.Role{}, errors.New(results[0])
		}
	}

	name := args["ROLE_NAME"]
	if strings.Contains(results[0], "already exists") {
		slog.Info(fmt.Sprintf("Role '%s' already exists: %s", name, results[0]))
	}

	ns := args["NAMESPACE"]
	return domain.NewRole(name, ns), nil
}

func parseRBACRoleBinding(args map[string]string, results ...string) (domain.RoleBinding, error) {
	if strings.Contains(results[0], "Error from server (Forbidden)") {
		// "command terminated with exit code 1: 'Error from server (Forbidden): roles.rbac.authorization.k8s.io \"nsadmin\" is forbidden: user \"system:serviceaccount:dev:developer\" (groups=[\"system:serviceaccounts\" \"system:serviceaccounts:dev\" \"system:authenticated\"]) is attempting to grant RBAC permissions not currently held:\n{APIGroups:[\"\"], Resources:[\"*\"], Verbs:[\"*\"]}\n'"
		if strings.Contains(results[0], "attempting to grant RBAC permissions not currently held") {
			return domain.RoleBinding{}, errors.New(results[0])
		}
	}

	roleName := args["ROLE_NAME"]
	subjectName := args["SUBJECT"]
	ns := args["NAMESPACE"]

	name := args["BINDING_NAME"]
	if strings.Contains(results[0], "already exists") {
		slog.Info(fmt.Sprintf("RoleBinding '%s' already exists: %s", name, results[0]))
	}
	maps := map[string]string{
		"ROLE_NAME": roleName,
		"SUBJECT":   subjectName,
		"NAMESPACE": ns,
	}
	for key, val := range maps {
		fieldVar := fmt.Sprintf("${%s}", key)
		name = strings.ReplaceAll(name, fieldVar, val)
	}

	roleID := fmt.Sprintf("ns/%s/role/%s", ns, roleName)
	subjectID := fmt.Sprintf("ns/%s/sa/%s", ns, subjectName)

	binding := domain.RoleBinding{
		K8sEntity: domain.K8sEntity{
			Name:      name,
			Namespace: ns,
			Kind:      "RoleBinding",
		},
		RoleID:     roleID,
		SubjectIDs: []string{subjectID},
	}

	return binding, nil
}

func parseK8sCronJob(args map[string]string, results ...string) (domain.CronJob, error) {
	numArgs := len(results)
	if numArgs == 0 {
		return domain.CronJob{}, fmt.Errorf("No data")
	}

	podName := results[0]

	if strings.Contains(podName, "created") {
		podName = strings.TrimSuffix(podName, " created")
	}
	podName = strings.ReplaceAll(podName, "cronjob.batch/", "")

	if len(results) > 1 {
		oldNsName := results[1]
		slog.Warn(fmt.Sprintf("parsing CronJob, old nsName was '%s'", oldNsName))
	}
	nsName := args["NAMESPACE"]

	ns := domain.NewNamespace(nsName)
	p := domain.NewPod(podName, nsName)

	if len(results) >= 3 {
		// TODO: marshal the podConfig
		var cfg domain.PodConfig
		err := json.Unmarshal([]byte(results[2]), &cfg)
		if err != nil {
			return domain.CronJob{}, fmt.Errorf("Failed to unmarshal PodConfig JSON: %w", err)
		}

		p.HostIPC = domain.AsProbBool(cfg.HostIPC)
		p.HostPID = domain.AsProbBool(cfg.HostPID)
		p.HostNetwork = domain.AsProbBool(cfg.HostNetwork)
		p.Privileged = domain.AsProbBool(cfg.Privileged)
	}

	// TODO: this should also add the new CronJob to the knowledge base, which owns this pod
	slog.Error(fmt.Sprintf("Creating new pod %s in namespace %s is not yet properly implemented! FIX NEEDED!", p.Name, ns.Name))

	cj := domain.NewCronJob(args["Name"], nsName)
	cj.Pod = p
	return cj, nil
}

type FileEntryParserFn func(string) (FileSystemEntry, error)

func getFileEntryParser(data string) FileEntryParserFn {
	if strings.HasPrefix(data, "total ") {
		return parseLSLine
	}

	return func(line string) (FileSystemEntry, error) {
		return FileSystemEntry{Name: line}, nil
	}
}

func parseFiles(data string) ([]FileSystemEntry, error) {
	// res := strings.Trim(data, "\n")
	files := strings.Split(data, "\n")

	parserFn := getFileEntryParser(data)

	var entries []FileSystemEntry
	for i, line := range files {
		// ls -l: long flag always prints `total <size>` at the top
		if i == 0 && strings.HasPrefix(line, "total ") {
			continue
		}
		entry, err := parserFn(line)
		if err != nil {
			return nil, fmt.Errorf("Failed to parse file entry '%s': %w", line, err)
		}
		// ignore standard navigation entries from `ls` command
		if entry.Name != "" && entry.Name != "./" && entry.Name != "../" {
			entries = append(entries, entry)
		}
	}
	return entries, nil
}

func parseLSLine(line string) (FileSystemEntry, error) {
	parts := strings.Fields(line)
	if len(parts) < 9 {
		return FileSystemEntry{}, nil
	}

	size, _ := strconv.ParseInt(parts[4], 10, 64)
	modTime, _ := time.Parse("Jan 02 15:04", fmt.Sprintf("%s %s", parts[5], parts[6]))
	name := parts[8]
	isExec := parts[0][3] == 'x' && strings.Contains(parts[0], "x")
	// part of -F flag in ls, to append '*' to executable files
	if isExec && strings.HasSuffix(name, "*") {
		name = strings.TrimSuffix(name, "*")
	}

	return FileSystemEntry{
		Name: name,
		Size: size,
		// Mode:    parseFileMode(parts[0]),
		ModTime: modTime,
		IsDir:   parts[0][0] == 'd' || strings.HasSuffix(name, "/"),
		IsExec:  isExec,
	}, nil
}

type FileSystemEntry struct {
	Name    string
	Size    int64
	Mode    os.FileMode
	ModTime time.Time
	IsDir   bool
	IsExec  bool
}
