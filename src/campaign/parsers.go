package campaign

import (
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"strconv"
	"strings"

	"github.com/Magier/Ran/domain"
	k8s "github.com/Magier/Ran/k8sclient"
	k8s_types "github.com/Magier/Ran/k8sclient/types"
)

func GetParser(parserName string) domain.ParserFn {
	switch parserName {
	// case "rawServiceaccountToken":
	// 	return HandleSaTokenRead
	case "environmentVariables":
		return HandleEnvVarResult
		// case "selfSubjectReview", "authCanI":
		// return HandleSelfSubjectReviewResult
	// case "newContainer":

	case "newRole":
		return HandleNewRole
	case "newRoleBinding":
		return HandleNewRoleBinding
	case "newCronJob":
		return HandleNewCronJob
	default:
		slog.Warn(fmt.Sprintf("Parser '%s' not implemented!", parserName))
	}
	return nil
}

func HandleEnvVarResult(ev domain.TTPExecuted, source domain.Entity, _ map[string]string, results ...string) (domain.Event, error) {
	if len(results) == 0 {
		return nil, errors.New("No environment variables received!")
	}
	stderr := results[1]
	if stderr != "" {
		return nil, errors.New(stderr)
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

	return domain.EnvVarsExtracted{
		Source: source,
		Vars:   vars,
	}, nil
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
			return ssrr, fmt.Errorf("Failed to unmarshal JSON: %w", err)
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

	// entitlements := make([]domain.RbacPermission, 0, len(result.Status.ResourceRules)+len(result.Status.NonResourceRules))
	// for _, rule := range ev.ResourceRules {
	// 	sa.Can = append(entitlements, domain.RbacPermission{
	// 		Verbs:         rule.Verbs,
	// 		ResourceTypes: rule.Resources,
	// 		ResourceNames: rule.ResourceNames,
	// 		ApiGroups:     rule.APIGroups,
	// 		Scope:         sa.GetNamespace(),
	// 	})
	// }

	// for _, rule := range ev.ResourceRules {
	// 	sa.Can = append(sa.Can, domain.RbacPermission{
	// 		Verbs:         rule.Verbs,
	// 		ResourceTypes: rule.Resources,
	// 		ResourceNames: rule.ResourceNames,
	// 		ApiGroups:     rule.APIGroups,
	// 		Scope:         "*",
	// 	})
	// }

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

func HandleNewPod(args map[string]string, results ...string) (domain.Event, error) {
	var cfg domain.PodConfig
	var nsName, podName string

	numArgs := len(results)
	if numArgs == 0 {
		return nil, fmt.Errorf("No data")
	}
	if numArgs != 3 {
		podName = args["Name"]
		nsName = args["Namespace"]
		cfg.NodeName = args["NodeName"]
		cfg.ServiceAccount = args["ServiceAccount"]

		hostIPC, _ := strconv.ParseBool(args["HostIPC"])
		cfg.HostIPC = hostIPC

		hostNetwork, _ := strconv.ParseBool(args["HostNetwork"])
		cfg.HostNetwork = hostNetwork

		hostPID, _ := strconv.ParseBool(args["HostPID"])
		cfg.HostPID = hostPID

		priv, _ := strconv.ParseBool(args["Privileged"])
		cfg.Privileged = priv

		hostPath := args["HostPath"]
		cfg.HostMounts = []domain.Mount{
			{MountPath: args["Mount"], Root: hostPath, ReadOnly: false, Flags: []string{"rw"}},
		}

	} else {
		// TODO: marshal the podConfig
		err := json.Unmarshal([]byte(results[2]), &cfg)
		if err != nil {
			return nil, fmt.Errorf("Failed to unmarshal PodConfig JSON: %w", err)
		}
	}

	// cfgJson := args[2].(domain.PodConfig)
	ns := domain.Namespace{Name: nsName}
	p := domain.NewPod(podName, nsName)

	p.HostIPC = domain.AsProbBool(cfg.HostIPC)
	p.HostPID = domain.AsProbBool(cfg.HostPID)
	p.HostNetwork = domain.AsProbBool(cfg.HostNetwork)
	p.Privileged = domain.AsProbBool(cfg.Privileged)
	p.ServiceAccountName = cfg.ServiceAccount
	p.NodeName = cfg.NodeName
	p.VolumeMounts = cfg.HostMounts

	slog.Error(fmt.Sprintf("Creating new pod %s in namespace %s is not yet properly implemented! FIX NEEDED!", p.Name, ns.Name))
	return domain.NewPodDeployed{
		Pod:       p,
		Namespace: ns,
	}, nil
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
	ns := domain.Namespace{Name: nsName}
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

func HandleNewRole(ev domain.TTPExecuted, source domain.Entity, ttpArgs map[string]string, results ...string) (domain.Event, error) {
	// TODO: check if the actual TTP execution failed, because the role already exists
	// -> overall, the intended effects are met, but it may be a confiict (e.g. name collision), for downstream TTPs
	if strings.Contains(results[0], "Error from server (Forbidden)") {
		// "command terminated with exit code 1: 'Error from server (Forbidden): roles.rbac.authorization.k8s.io \"nsadmin\" is forbidden: user \"system:serviceaccount:dev:developer\" (groups=[\"system:serviceaccounts\" \"system:serviceaccounts:dev\" \"system:authenticated\"]) is attempting to grant RBAC permissions not currently held:\n{APIGroups:[\"\"], Resources:[\"*\"], Verbs:[\"*\"]}\n'"
		if strings.Contains(results[0], "attempting to grant RBAC permissions not currently held") {
			return nil, errors.New(results[0])
		}
	}

	name := ev.Args["ROLE_NAME"]
	if strings.Contains(results[0], "already exists") {
		slog.Info(fmt.Sprintf("Role '%s' already exists: %s", name, results[0]))
	}

	var ns string
	var creator domain.ServiceAccount

	if sa, ok := ev.Target.(domain.ServiceAccount); ok {
		ns = sa.GetNamespace()
		creator = sa
	}

	role := domain.Role{
		K8sEntity: domain.K8sEntity{
			Name:      name,
			Namespace: ns,
			Kind:      "Role",
		},
	}

	myId := role.GetId()
	var _ = myId

	return domain.NewK8sResourceCreated{
		Resource:  role,
		CreatorID: creator.GetId(),
	}, nil
}

func HandleNewRoleBinding(ev domain.TTPExecuted, source domain.Entity, ttpArgs map[string]string, results ...string) (domain.Event, error) {
	if strings.Contains(results[0], "Error from server (Forbidden)") {
		// "command terminated with exit code 1: 'Error from server (Forbidden): roles.rbac.authorization.k8s.io \"nsadmin\" is forbidden: user \"system:serviceaccount:dev:developer\" (groups=[\"system:serviceaccounts\" \"system:serviceaccounts:dev\" \"system:authenticated\"]) is attempting to grant RBAC permissions not currently held:\n{APIGroups:[\"\"], Resources:[\"*\"], Verbs:[\"*\"]}\n'"
		if strings.Contains(results[0], "attempting to grant RBAC permissions not currently held") {
			return nil, errors.New(results[0])
		}
	}

	name := ev.Args["BINDING_NAME"]
	if strings.Contains(results[0], "already exists") {
		slog.Info(fmt.Sprintf("RoleBinding '%s' already exists: %s", name, results[0]))
	}

	ns := ev.Args["NAMESPACE"]
	roleID := fmt.Sprintf("ns/%s/role/%s", ns, ev.Args["ROLE_NAME"])
	subjectID := fmt.Sprintf("ns/%s/sa/%s", ns, ev.Args["SUBJECT"])

	binding := domain.RoleBinding{
		K8sEntity: domain.K8sEntity{
			Name:      name,
			Namespace: ns,
			Kind:      "RoleBinding",
		},
		RoleID:     roleID,
		SubjectIDs: []string{subjectID},
	}

	// TODO infer the proper creator
	// creatorName := fmt.Sprintf("ns/%s/sa/%s", ns, ev.Args["TOKEN"])
	return domain.NewK8sResourceCreated{
		Resource: binding,
		// CreatorID: creatorName,
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

func ParseEffect(effect string, source domain.Entity, args map[string]string, results ...string) (NewFacts, RemovedFacts) {
	if len(results) == 0 {
		slog.Warn("Can't parse effect %s because there are no arguments")
		return NewFacts{}, RemovedFacts{}
	}

	// alreadyExists := false
	if strings.Contains(results[0], "already exists") {
		// alreadyExists = true
		slog.Info(fmt.Sprintf("Parsing Effect: entity '%s' already exists", effect))
	}

	isRemoveEffect := strings.HasPrefix(effect, "delete")
	effect = strings.TrimPrefix(effect, "delete ")

	res := results[0]
	entities := []domain.Entity{}
	relations := []domain.Relation{}
	switch strings.ToLower(effect) {
	// TODO: set these 'attribute' effects via reflection
	case "target.ip":
		if pod, ok := source.(domain.Pod); ok {
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
			pod.IPs = ips
			entities = append(entities, pod)
		}
	case "target.hasbinary":
		// "target.hasBinary"
		if pod, ok := source.(domain.Pod); ok {
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
				dstPath = "❌"
				// TODO: get the path from the SRC_PATH?
				slog.Warn("No DST_PATH provided, and extraction from SRC_PATh is not yet implemented!")
			}
			pod.Binaries[binaryName] = dstPath
			entities = append(entities, pod)
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
	case "k8s.selfsubjectrulesreview":
		ssrr, err := parseSelfSubjectRulesReview(results...)
		if err != nil {
			slog.Error(fmt.Sprintf("Could not parse PodList: %v", err))
		} else {
			sa, ok := source.(domain.ServiceAccount)
			if !ok {
				slog.Warn("the source of the SubjectReviewResult is not a valid ServiceAccount!")
			} else {
				ssrr.ServiceAccount = sa
				ssrr.TokenName = sa.GetName()
			}
		}
		// TODO: temporary workaround to treat SelfSubjectRulesReview as an entity, so it's processed in the analyzer
		entities = append(entities, ssrr)
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
		ev, err := HandleNewPod(args)
		newPod := ev.(domain.NewPodDeployed)
		if err != nil {
			entities = append(entities, newPod.Pod, newPod.Namespace)
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
	case "k8s.nodelist":
		nodeList, err := k8s.ParseNodeList(res)
		if err != nil {
			slog.Error(fmt.Sprintf("Failed to parse NodeList: %v", err))
		} else {
			for _, node := range nodeList.Items {
				entities = append(entities, domain.NewK8sNodeFromK8sSpec(node))
			}
		}
	case "linux.mounts":
		if pod, ok := source.(domain.Pod); ok {
			mounts, err := parseLinuxMounts(res)
			if err != nil {
				slog.Error(fmt.Sprintf("Failed to parse Linux mounts: %v", err))
			} else {
				pod.VolumeMounts = append(pod.VolumeMounts, mounts...)
				entities = append(entities, pod)
			}
		}
	}

	newFacts := NewFacts{}
	removedFacts := RemovedFacts{}
	if isRemoveEffect {
		removedFacts = RemovedFacts{Entities: entities, Relations: relations}
	} else {
		newFacts = NewFacts{Entities: entities, Relations: relations}
	}

	return newFacts, removedFacts
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
		ID:        id,
		ParentID:  parentID,
		Root:      fields[3],
		MountPath: fields[4],
		Type:      fields[7],
		ReadOnly:  isReadOnly,
		Flags:     flags,

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
		Root:      device,
		MountPath: mountPath,
		Type:      fsType,
		ReadOnly:  readOnly,
		Flags:     options,
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
