package domain

import (
	"fmt"
	"log/slog"
	"net"
	"strings"

	"encoding/json"

	"golang.org/x/text/cases"
	"golang.org/x/text/language"
	appsv1 "k8s.io/api/apps/v1"
	v1 "k8s.io/api/core/v1"
)

type Protocol string

const (
	ANY   Protocol = "ANY"
	TCP   Protocol = "TCP"
	UDP   Protocol = "UDP"
	HTTP  Protocol = "HTTP"
	HTTPS Protocol = "HTTPS"
	DNS   Protocol = "DNS"
	mTLS  Protocol = "mTLS"
)

type Condition interface {
	Satisfies(Condition) bool
	IsSet() bool
}

type Requirements struct {
	Kind           IsOfKind               `yaml:"kind"`
	AccessLevel    AccessLevel            `yaml:"accessLevel" json:"accessLevel"`
	RBACPermission RBACPermission         `yaml:"rbac" json:"rbac,omitzero"`
	State          State                  `yaml:"-" json:"-"` // check for existing entities
	Exists         EntitiesExists         // relates to the state
	OtherFields    map[string]interface{} `yaml:",inline"` // Inline captures untagged fields
}

func (r Requirements) Satisfied(target Entity, accessLevel AccessLevel, state State) bool {
	var targetKind IsOfKind
	if target != nil {
		targetKind = IsOfKind(target.GetKind())
	}
	if r.Kind != "" && r.Kind != targetKind {
		return false
	}
	if !accessLevel.Satisfies(r.AccessLevel) {
		return false
	}

	if r.RBACPermission.Verb != "" {
		_, ok := state.Entitlements[r.RBACPermission.String()]
		if !ok {
			// TODO: temporary workaround to check for wildcard permissions
			nsAdmin := RBACPermission{Verb: "*", ResourceType: "*"}
			if _, ok := state.Entitlements[nsAdmin.String()]; !ok {
				return false
			}
		}
	}

	if len(r.Exists) > 0 {
		if !state.Satisfies(r.Exists) {
			return false
		}
	}
	return true
}

type IsOfKind string

func (k IsOfKind) Satisfies(r Condition) bool {
	if kind, ok := r.(IsOfKind); ok {
		return k == kind
	}
	return false
}
func (k IsOfKind) IsSet() bool {
	return k != ""
}

var _ Condition = (*IsOfKind)(nil)

type State struct {
	Entitlements map[string][]string `json:"entitlements,omitempty"`
	EntityCounts map[string]int      `json:"entityCounts,omitempty"`
}

func (s State) Satisfies(r Condition) bool {
	if entityKinds, ok := r.(EntitiesExists); ok {
		for _, k := range entityKinds {
			numExists, existsOk := s.EntityCounts[strings.ToLower(string(k))]
			return existsOk && numExists > 0
		}
	}
	return false
}
func (s State) IsSet() bool {
	return false
}
func (s State) Update(key string, numChange int) State {
	prevNum, exists := s.EntityCounts[key]
	if !exists {
		prevNum = 0
	}

	s.EntityCounts[key] = prevNum + numChange
	return s
}

var _ Condition = (*State)(nil)

type AccessLevel struct {
	User  int // 0 = none, 1 = user, 2 = root
	Level int // 0 = none, 1 = read, 2 = exec
}

func (lvl AccessLevel) Satisfies(requirement Condition) bool {
	if r, ok := requirement.(AccessLevel); ok {
		return r.User <= lvl.User && r.Level <= lvl.Level
	}
	return false
}
func (lvl AccessLevel) IsSet() bool {
	return lvl != NoAccess
}

var _ Condition = (*AccessLevel)(nil)

func (lvl AccessLevel) String() string {
	switch lvl {
	case UserRead:
		return "user-read"
	case UserExec:
		return "user-exec"
	case RootRead:
		return "root-read"
	case RootExec:
		return "root-exec"
	}
	return ""
}

func parseAccessLevel(level string) AccessLevel {
	switch level {
	case "user-read":
		return UserRead
	case "user-exec":
		return UserExec
	case "root-read":
		return RootRead
	case "root-exec":
		return RootExec
	default:
		return NoAccess
	}
}

// Implements the Unmarshaler interface of the yaml pkg.
func (e *AccessLevel) UnmarshalYAML(unmarshal func(interface{}) error) error {
	var level string
	err := unmarshal(&level)
	if err != nil {
		return err
	}
	*e = parseAccessLevel(level)
	return nil
}

func (lvl AccessLevel) MarshalJSON() ([]byte, error) {
	return json.Marshal(lvl.String())
}

var (
	NoAccess = AccessLevel{User: 0, Level: 0}
	UserRead = AccessLevel{User: 1, Level: 1}
	UserExec = AccessLevel{User: 1, Level: 2}
	RootRead = AccessLevel{User: 2, Level: 1}
	RootExec = AccessLevel{User: 2, Level: 2}
)

type EntitiesExists []string

// // Implements the Unmarshaler interface of the yaml pkg.
// func (e *EntitiesExists) UnmarshalYAML(unmarshal func(interface{}) error) error {
// 	var entities []string
// 	err := unmarshal(&entities)
// 	if err != nil {
// 		return err
// 	}
// 	*e = entities
// 	return nil
// }

func (e EntitiesExists) Satisfies(requirement Condition) bool {
	return false
}
func (e EntitiesExists) IsSet() bool {
	return len(e) > 0
}
func (e EntitiesExists) String() string {
	if len(e) > 0 {
		entities := make([]string, len(e))
		for i, entity := range e {
			entities[i] = string(entity)
		}
		return "∃ " + strings.Join(entities, ", ")
	}
	return ""
}

var _ Condition = (*AccessLevel)(nil)

type IdentityType string

type Listener struct {
	ID         string
	Port       uint
	Protocol   Protocol
	Redirector string
	IP         net.IP
}

// GetKind implements Entity.
func (l Listener) GetKind() string {
	return "Listener"
}

// GetName implements Entity.
func (l Listener) GetName() string {
	return fmt.Sprintf("listener_%d", l.Port)
}

func (l Listener) GetId() string {
	return l.ID
}

var _ Entity = (*Listener)(nil)

type Workload interface {
	Entity
	Namespaced
	GetPods() []Pod
}

type AbstractWorkload struct {
	K8sEntity
	// NamespacedResource
	ResourceOwner
}

var _ Workload = (*AbstractWorkload)(nil)

func (wl AbstractWorkload) GetId() string {
	return fmt.Sprintf("ns/%s/wl/%s", wl.GetNamespace(), wl.GetName())
}

func (wl AbstractWorkload) GetKind() string {
	return "AbstractWorkload"
}

//	func (wl AbstractWorkload) GetPods() []Pod {
//		return wl.Pods
//	}
func (wl AbstractWorkload) IsAbstract() bool {
	return true
}

type ResourceOwner struct {
	Pods []Pod `json:"pods,omitzero"` // Pods that are owned by this workload
}

func (w ResourceOwner) GetPods() []Pod {
	return w.Pods
}

type Entity interface {
	GetId() string
	GetName() string
	GetKind() string
}

type Ownable interface {
	GetId() string
	GetOwner() (OwnerRef, bool)
	SetOwner(name, kind string) OwnerRef
}

type System interface {
	Entity
	SetEnvironmentVariables(map[string]string)
	GetEnvironmentVariables() map[string]string
	GetAccessLevel() AccessLevel
	SetAccessLevel(AccessLevel)
	SetIPs(ips []net.IPAddr)
}

type UnknownSystem struct {
	*SystemImpl
}

func (s UnknownSystem) GetId() string {
	return "???"
}
func (s UnknownSystem) GetName() string {
	return s.HostName
}

func (s UnknownSystem) GetKind() string {
	return "UnknownSystem"
}

func NewSystem(hostname, os string, accessLevel AccessLevel) UnknownSystem {
	return UnknownSystem{
		SystemImpl: &SystemImpl{
			HostName:    hostname,
			OS:          os,
			AccessLevel: accessLevel,
			IPs:         []net.IPAddr{},
			EnvVars:     make(map[string]string),
			Binaries:    make(map[string]string),
			Files:       []string{},
			Mounts:      []Mount{},
		},
	}
}

var _ Entity = (*UnknownSystem)(nil)

type SystemImpl struct {
	HostName    string            `json:"hostName,omitzero"` // Hostname of the system
	OS          string            `json:"os,omitzero"`       // Operating system of the system (e.g., Linux, Windows)
	IPs         []net.IPAddr      `json:"ips,omitzero"`      // List of IP addresses associated with the system
	EnvVars     map[string]string `json:"envVars,omitzero,omitempty"`
	Binaries    map[string]string `json:"binaries,omitempty"` // mapping of binary names to their paths
	Files       []string          `json:"files,omitzero"`     // List of files on the node
	Mounts      []Mount           `json:"mounts,omitzero"`
	AccessLevel AccessLevel       `json:"accessLevel,omitzero"` // Access level of the system (e.g., user, root)
}

func (s *SystemImpl) GetAccessLevel() AccessLevel {
	return s.AccessLevel
}

func (s *SystemImpl) SetAccessLevel(level AccessLevel) {
	s.AccessLevel = level
}

func (s *SystemImpl) GetEnvironmentVariables() map[string]string {
	if s.EnvVars == nil {
		s.EnvVars = make(map[string]string)
	}
	return s.EnvVars
}

func (s *SystemImpl) SetEnvironmentVariables(vars map[string]string) {
	s.EnvVars = vars
}

func (s *SystemImpl) SetIPs(ips []net.IPAddr) {
	s.IPs = ips
}

// func (s System) GetId() string {
// 	return "system/" + s.Name
// }
// func (s System) GetName() string {
// 	return s.Name
// }

// func (s System) GetKind() string {
// 	return "System"
// }

type C2System struct {
	Kind string
	Name string
	IPs  []net.IP
}

func (s C2System) GetId() string {
	return "c2/" + s.Name
}
func (s C2System) GetName() string {
	return s.Name
}

func (s C2System) GetKind() string {
	return "C2"
}

type Namespaced interface {
	GetNamespace() string
	IsNamespaced() bool
}

type EntityPlaceholder interface {
	IsAbstract() bool
}

func GenerateId(name, kind, ns string) string {
	kindShortName := GetResourceShortName(kind)
	id := "/" + kindShortName + "/" + name
	// if it doesn't start with "ns/" then ID has pattern "/kind", which equate to a clusterwide resource
	if ns != "" {
		id = "ns/" + ns + id
	}
	return id
}

type OwnerRef struct {
	Name string
	Kind string
	Uid  string
}

type K8sEntity struct {
	Id          string
	Name        string
	Kind        string
	Namespace   string            `json:"namespace,omitzero"` // Namespace is optional, so it can be empty
	Labels      map[string]string `json:"labels,omitzero,omitempty"`
	Annotations map[string]string `json:"annotations,omitzero,omitempty"`
	CreatedAt   string            `json:"createdAt,omitzero"` // RFC3339 format
	Owner       OwnerRef          `json:"owner,omitzero"`
	// AccessLevel AccessLevel       `json:"accessLevel,omitzero"` // AccessLevel is a custom type that can be marshaled to/from JSON
}

func NewK8sEntity(name, kind, namespace string) K8sEntity {
	return K8sEntity{
		Name:      name,
		Kind:      kind,
		Namespace: namespace,
		// AccessLevel: NoAccess,
		Labels:      make(map[string]string),
		Annotations: make(map[string]string),
		// TODO set createdAt here
	}
}
func K8sEntityFromId(id string) K8sEntity {
	parts := strings.Split(id, "/")
	n := len(parts)
	// IDs have format ns/<ns>/<kind>/<name>
	// where the ns information is opitonal

	name := parts[n-1]
	kind := parts[n-2]
	var ns string

	if n > 2 {
		ns = parts[n-3]
	}

	// check `kubectl api-resources` as reference for shortnames
	kindMap := map[string]string{
		// "pod":     "Pod",
		// "secret":  "Secret",
		// "node":    "Node",
		// "role":        "Role",
		"sa":      "ServiceAccount",
		"ns":      "Namespace",
		"wl":      "AbstractWorkload",
		"rs":      "ReplicaSet",
		"sts":     "StatefulSet",
		"ds":      "DaemonSet",
		"svc":     "Service",
		"deploy":  "Deployment",
		"cronjob": "CronJob",
		"rb":      "RoleBinding",
		"cr":      "ClusterRole",
		"crb":     "ClusterRoleBinding",
		// "c2":       "C2",
		// "system":   "System",
		// "listener": "Listener",
		// "session":  "Session",
	}
	if fullKind, ok := kindMap[strings.ToLower(kind)]; ok {
		kind = fullKind
	} else {
		kind = cases.Title(language.English, cases.NoLower).String(kind) // Title case the kind
	}
	// if n > 4 {
	// 	cluster = parts[n-5]
	// }

	return K8sEntity{
		Id:        id,
		Name:      name,
		Kind:      kind,
		Namespace: ns,
	}
}

func (e K8sEntity) GetId() string {
	return GenerateId(e.Name, e.Kind, e.Namespace)
}

func (e K8sEntity) GetName() string {
	return e.Name
}

func (e K8sEntity) GetKind() string {
	return e.Kind
}

func (e K8sEntity) GetLabel(label string) (string, bool) {
	if e.Labels != nil {
		v, ok := e.Labels[label]
		return v, ok
	}
	return "", false
}
func (e K8sEntity) GetOwner() (OwnerRef, bool) {
	if e.Owner.Name != "" {
		return e.Owner, true
	}
	return OwnerRef{}, false
}
func (e K8sEntity) SetOwner(name, kind string) OwnerRef {
	return OwnerRef{
		Name: name,
		Kind: kind,
	}
}

func (e K8sEntity) GetNamespace() string {
	return e.Namespace
}

func (e K8sEntity) IsNamespaced() bool {
	return true
}

// type NamespacedResource struct {
// 	Namespace string
// }

// func (n NamespacedResource) GetNamespace() string {
// 	return n.Namespace
// }

const TheOnlyClusterId string = "cluster"

type Cluster struct {
	Name    string
	Address string
}

// GetId implements Entity.
func (c Cluster) GetId() string {
	return TheOnlyClusterId // TODO: very naive assumption of having just 1 cluster for now
}

// GetKind implements Entity.
func (c Cluster) GetKind() string {
	return "Cluster"
}

// GetName implements Entity.
func (c Cluster) GetName() string {
	return c.Name
}

var _ Entity = (*Cluster)(nil)

type ApiServer struct {
	Pod
	CAData     []byte
	ExternalIP net.IPAddr
}

type Namespace struct {
	Name        string
	EnforcedPSS string
	WarnPSS     string
	AuditPSS    string
}

var _ Entity = (*Namespace)(nil)

func (ns Namespace) GetId() string {
	return "ns/" + ns.Name
}

func (ns Namespace) GetName() string {
	return ns.Name
}

func (ns Namespace) GetKind() string {
	return "Namespace"
}

type RBACPermission struct {
	Verb         string `json:"verb,omitzero"`
	ResourceName string `json:"resourceName,omitzero"`
	ResourceType string `json:"resourceType,omitzero"`
	APIGroup     string `json:"apiGroup,omitzero"`
	Scope        string `json:"scope,omitzero"` // "" is invalid, "*" =cluster-wide, any string = namespaces
}

func (p RBACPermission) String() string {
	s := fmt.Sprintf("%s %s", p.Verb, p.ResourceType)
	if p.ResourceName != "" {
		sep := ""
		if p.ResourceType != "" {
			sep = "/"
		}
		s += sep + p.ResourceName
	}
	return s
}

// Implements the Unmarshaler interface of the yaml pkg.
func (p *RBACPermission) UnmarshalYAML(unmarshal func(interface{}) error) error {
	var s string
	if err := unmarshal(&s); err != nil {
		return err
	}
	fields := strings.Fields(s)

	if len(fields) < 2 {
		return fmt.Errorf("RBACPermission must have at least 2 fields: verb and resource type, got %d fields", len(fields))
	}

	verb := fields[0]
	resourceType := NormalizeResourceType(fields[1])

	// TODO: resource names or scopes are not supported yet

	*p = RBACPermission{
		Verb:         verb,
		ResourceType: resourceType,
	}
	return nil
}

func (p RBACPermission) Satisfies(r Condition) bool {
	return false
}

func (p RBACPermission) IsSet() bool {
	return false
}

// type RbacPermission struct {
// 	Verbs         []string
// 	ResourceTypes []string
// 	ResourceNames []string
// 	ApiGroups     []string
// 	Scope         string // "" is invalid, "*" =cluster-wide, any string = namespaces
// }

type Identity interface {
	GetId() string
	GetToken() string
	Can(verb, res string) bool
	GetEntitlements() []RBACPermission
}

type User struct {
	Name         string
	Kind         IdentityType
	IsAdmin      bool
	CertData     []byte
	KeyData      []byte
	Entitlements []RBACPermission
	Token        string
}

// GetId implements Entity.
func (user User) GetId() string {
	return user.Name
}

// GetKind implements Entity.
func (user User) GetKind() string {
	return string(user.Kind)
}

// GetName implements Entity.
func (user User) GetName() string {
	return user.Name
}

func (user User) GetEntitlements() []RBACPermission {
	return user.Entitlements
}

var _ (Identity) = (*User)(nil)

// GetToken implements Identity.
func (user User) GetToken() string {
	return user.Token
}

// Can implements Identity.
func (user User) Can(verb, resource string) bool {
	for _, perm := range user.Entitlements {
		// for _, v := range perm.Verbs {
		// TODO: properly filter for scope, resource name/type + wildcards
		if perm.Verb == "*" {
			return true
		}
		// }
	}
	return false
}

type Mount struct {
	ID         int      `json:"id,omitzero"`         // Unique identifier for the mount
	ParentID   int      `json:"parentId,omitzero"`   // ID of the parent mount, if any
	Name       string   `json:"name,omitzero"`       // Name of the volume mount
	MountPath  string   `json:"mountPath,omitzero"`  // Path in the container where the volume is mounted
	Root       string   `json:"hostPath,omitzero"`   // Path on the host where the volume is mounted
	Type       string   `json:"type,omitzero"`       // Type of the volume mount (e.g. "hostPath", "emptyDir", "configMap", etc.)
	ReadOnly   bool     `json:"readOnly,omitzero"`   // Whether the volume is mounted as read-only
	IsHostPath bool     `json:"isHostPath,omitzero"` // Whether the source is from the host system
	Flags      []string `json:"flags,omitzero"`      // e.g. "z", "Z"
}

type Pod struct {
	K8sEntity
	*SystemImpl // SystemImpl contains system-level information like OS, IPs, environment variables, etc.
	// IPs                          []net.IPAddr      `json:"ips,omitzero"`
	// EnvVars                      map[string]string `json:"envVars,omitzero"`
	// Binaries map[string]string `json:"binaries,omitempty"` // mapping of binary names to their paths
	// Files    []string          `json:"files,omitzero"`     // List of files on the node
	// Mounts   []Mount           `json:"volumeMounts,omitzero"`
	// NamespacedResource
	Spec                         v1.PodSpec     `json:"spec,omitzero"`
	ServiceAccountName           string         `json:"serviceAccountName,omitzero"`
	AutomountServiceAccountToken ProbBool       `json:"automountServiceAccountToken,omitzero"`
	HostName                     string         `json:"hostName,omitzero"`
	NodeName                     string         `json:"nodeName,omitzero"`
	Privileged                   ProbBool       `json:"privileged,omitzero"`
	HostPID                      ProbBool       `json:"hostPID,omitzero"`
	HostIPC                      ProbBool       `json:"hostIPC,omitzero"`
	HostNetwork                  ProbBool       `json:"hostNetwork,omitzero"`
	ReadOnlyRootFilesystem       ProbBool       `json:"readOnlyRootFilesystem,omitzero"`
	VolumeMounts                 []Mount        `json:"volumeMounts,omitzero"`
	HostPaths                    []string       `json:"hostPaths,omitzero"` // Paths on the host that are mounted into the pod
	Containers                   []v1.Container `json:"containers,omitzero"`
	HostIP                       net.IPAddr     `json:"hostIP,omitzero"`
	Phase                        string         `json:"phase,omitzero"`
	IsRunning                    bool           `json:"isRunning"`
	// Devices                      []string          `json:"devices,omitzero"`
}

var _ Namespaced = (*Pod)(nil)
var _ System = (*Pod)(nil)

// func (p Pod) GetAccessLevel() AccessLevel {
// 	return p.System.GetAccessLevel()
// }
// func (p *Pod) SetEnvironmentVariables(vars map[string]string) {
// 	p.System.SetEnvironmentVariables(vars)
// }

type PodConfig struct {
	Image          string
	Command        string
	Args           []string
	HostIPC        bool
	HostPID        bool
	HostNetwork    bool
	Privileged     bool
	NodeName       string
	HostMounts     []Mount
	ServiceAccount string
}

func NewPod(name, ns string) Pod {
	entity := NewK8sEntity(name, "Pod", ns)
	return Pod{
		K8sEntity: entity,
		SystemImpl: &SystemImpl{
			Binaries: make(map[string]string),
		},
		AutomountServiceAccountToken: NewProbBool(),
		Privileged:                   NewProbBool(),
		HostPID:                      NewProbBool(),
		HostIPC:                      NewProbBool(),
		HostNetwork:                  NewProbBool(),
		ReadOnlyRootFilesystem:       NewProbBool(),
		IsRunning:                    true,
	}
}

func getVolumeMountsFromSpec(pod v1.Pod, mountName string) (v1.VolumeMount, bool) {
	for _, container := range pod.Spec.Containers {
		for _, volumeMount := range container.VolumeMounts {
			if volumeMount.Name == mountName {
				return volumeMount, true
			}
		}
	}

	return v1.VolumeMount{}, false
}

func NewPodFromK8sSpec(p v1.Pod) Pod {
	entity := NewK8sEntity(p.ObjectMeta.Name, "Pod", p.Namespace)
	isPriv := AsProbBool(false)

	for _, c := range p.Spec.Containers {
		if c.SecurityContext != nil {
			priv := c.SecurityContext.Privileged
			if priv != nil {
				isPriv = AsProbBool(*priv)
			}
		}
	}

	// TODO: handle multiple containers in the pod
	var readOnlyRootFS ProbBool
	if p.Spec.Containers[0].SecurityContext != nil && p.Spec.Containers[0].SecurityContext.ReadOnlyRootFilesystem != nil {
		readOnlyRootFS = AsProbBool(*p.Spec.Containers[0].SecurityContext.ReadOnlyRootFilesystem)
	}

	mounts := make([]Mount, 0, len(p.Spec.Volumes))
	for _, v := range p.Spec.Volumes {
		mount := Mount{
			Name: v.Name,
			// MountPath: v.HostPath.Path, // Assuming HostPath for simplicity
			// Type:      string(v.VolumeSource.HostPath.Type),
			// ReadOnly:  v.HostPath != nil && v.HostPath.ReadOnly,
		}
		if v.HostPath != nil {
			if vm, ok := getVolumeMountsFromSpec(p, v.Name); ok {
				mount.Root = v.HostPath.Path
				mount.IsHostPath = true
				mount.MountPath = vm.MountPath
				mount.ReadOnly = vm.ReadOnly
			}
		}
		// if v.VolumeSource.Projected != nil {
		// 	mount.Type = "projected"
		//}

		// TODO: add support for ProjectedVolumes
		// if v.EmptyDir != nil {
		// 	mount.Type = "emptyDir"
		// } else if v.ConfigMap != nil {
		// 	mount.Type = "configMap"
		// 	mount.Flags = v.ConfigMap.Items[0].Key // Example, assuming single item
		// }
		mounts = append(mounts, mount)
	}
	slog.Warn(">> Pod volumes are not fully supported yet!!")

	return Pod{
		K8sEntity: entity,
		SystemImpl: &SystemImpl{
			Binaries: make(map[string]string),
			IPs:      []net.IPAddr{{IP: net.ParseIP(p.Status.PodIP)}},
		},
		HostPID:                AsProbBool(p.Spec.HostPID),
		HostIPC:                AsProbBool(p.Spec.HostIPC),
		HostNetwork:            AsProbBool(p.Spec.HostNetwork),
		ReadOnlyRootFilesystem: readOnlyRootFS,
		NodeName:               p.Spec.NodeName,
		VolumeMounts:           mounts,
		Privileged:             isPriv,
		HostIP:                 net.IPAddr{IP: net.ParseIP(p.Status.HostIP)},
		Containers:             p.Spec.Containers,
		Phase:                  string(p.Status.Phase),
		IsRunning:              p.Status.Phase == v1.PodRunning,
		// HostPaths:              []string{},
	}
}

type Deployment struct {
	K8sEntity
	// NamespacedResource
	ResourceOwner
}

func NewDeployment(name, ns string) Deployment {
	return Deployment{
		K8sEntity: K8sEntity{
			Name:      name,
			Namespace: ns,
			Kind:      "Deployment",
		},
	}
}
func NewDeploymentFromK8sSpec(p appsv1.Deployment) Deployment {
	entity := NewK8sEntity(p.ObjectMeta.Name, "Deployment", p.Namespace)
	// isPriv := NewProbBool(false)

	// for _, c := range p.Spec.Template.Spec.Containers {
	// if c.SecurityContext != nil {
	// priv := c.SecurityContext.Privileged
	// if priv != nil {
	// 	isPriv = NewProbBool(*priv)
	// }
	// }
	// }

	return Deployment{
		K8sEntity: entity,
		// HostPID:     NewProbBool(p.Spec.Template.Spec.HostPID),
		// HostIPC:     NewProbBool(p.Spec.Template.Spec.HostIPC),
		// HostNetwork: NewProbBool(p.Spec.Template.Spec.HostNetwork),
		// NodeName:    p.Spec.Template.Spec.NodeName,
		// Privileged:  isPriv,
		// IPs:         []net.IPAddr{{IP: net.ParseIP(p.Status.PodIP)}},
		// Containers:  p.Spec.Template.Spec.Containers,
	}
}

type CronJob struct {
	K8sEntity
	// NamespacedResource
	ResourceOwner
}

func NewCronJob(name, ns string) CronJob {
	return CronJob{
		K8sEntity: K8sEntity{
			Name:      name,
			Namespace: ns,
			Kind:      "CronJob",
		},
	}
}

type Service struct {
	K8sEntity
	// NamespacedResource
	Targets []string
	Host    string
	FQDN    string
	Ports   map[string]int
}

type ReplicaSet struct {
	K8sEntity
	ResourceOwner
	// NamespacedResource
}

type StatefulSet struct {
	K8sEntity
	ResourceOwner
	// NamespacedResource
}

type DaemonSet struct {
	K8sEntity
	ResourceOwner
	// NamespacedResource
}

type K8sNode struct {
	UID string
	K8sEntity
	ResourceOwner
	*SystemImpl
}

func (n K8sNode) IsNamespaced() bool { return false }

func NewK8sNode(name string) K8sNode {
	entity := NewK8sEntity(name, "Node", "")
	return K8sNode{
		K8sEntity: entity,
		SystemImpl: &SystemImpl{
			Binaries: make(map[string]string),
			EnvVars:  make(map[string]string),
		},
	}
}

func NewK8sNodeFromK8sSpec(n v1.Node) K8sNode {
	entity := NewK8sEntity(n.ObjectMeta.Name, "Node", "")
	return K8sNode{
		K8sEntity: entity,
		UID:       string(n.ObjectMeta.UID),
		SystemImpl: &SystemImpl{
			Binaries: make(map[string]string),
			EnvVars:  make(map[string]string),
		},
	}
}

func (n K8sNode) GetId() string {
	return fmt.Sprintf("node/%s", n.GetName())
}

type Role struct {
	K8sEntity
}

func (role Role) GetKind() string {
	return "Role"
}

type RoleBinding struct {
	K8sEntity
	RoleID     string
	SubjectIDs []string
}

func (role RoleBinding) GetKind() string {
	return "RoleBinding"
}

type Asset interface {
}

// JWT RFC: https://datatracker.ietf.org/doc/html/rfc7519#section-4
type JWToken struct {
	Subject        string   `json:"sub"`
	Audience       []string `json:"aud"`
	Issuer         string   `json:"iss"`
	ExpiresAt      int      `json:"exp"`
	IssuedAt       int      `json:"iat"`
	NotValidBefore int      `json:"nbf"`
	Raw            string
}

type ResourceRef struct {
	Name string `json:"name"`
	UID  string `json:"uid"`
}

type ServiceAccountToken struct {
	JWToken
	Kubernetes struct {
		Namespace      string      `json:"namespace"`
		Pod            ResourceRef `json:"pod,omitempty"`
		Node           ResourceRef `json:"node,omitempty"`
		ServiceAccount ResourceRef `json:"serviceaccount,omitempty"`
		Warnafter      int         `json:"warnafter"`
	} `json:"kubernetes.io"`
	IsBound bool
	// TODO verify if issuer is indicator of K8s API server?
	// PodUid             string
	Raw string
}

func (s ServiceAccountToken) GetId() string {
	// Use the ServiceAccount name and namespace to create a unique ID
	return fmt.Sprintf("ns/%s/sa/%s/token", s.Kubernetes.Namespace, s.Kubernetes.ServiceAccount.Name)
}

func (s ServiceAccountToken) GetKind() string {
	return "ServiceAccountToken"
}

func (s ServiceAccountToken) GetName() string {
	return fmt.Sprintf("sa-token-%s", s.Kubernetes.ServiceAccount.Name)
}

var _ Asset = (*ServiceAccountToken)(nil)

type ServiceAccount struct {
	K8sEntity
	Token         ServiceAccountToken `json:"token,omitzero"`
	SecretNames   []string            `json:"secretNames,omitzero"`
	Entitelements []RBACPermission    `json:"can,omitzero"`
}

func NewServiceAccount(name, ns string) ServiceAccount {
	return ServiceAccount{
		K8sEntity: K8sEntity{
			Name:      name,
			Namespace: ns,
			Kind:      "ServiceAccount",
		},
		Entitelements: make([]RBACPermission, 0),
	}
}

func (sa ServiceAccount) GetId() string {
	return fmt.Sprintf("ns/%s/sa/%s", sa.GetNamespace(), sa.GetName())
}

func (sa ServiceAccount) GetKind() string {
	return "ServiceAccount"
}
func NewServiceAccountFromK8sSpec(sa v1.ServiceAccount) ServiceAccount {
	entity := NewK8sEntity(sa.ObjectMeta.Name, "ServiceAccount", sa.Namespace)

	secretNames := make([]string, len(sa.Secrets))
	for i, secret := range sa.Secrets {
		secretNames[i] = secret.Name
	}
	return ServiceAccount{
		K8sEntity:   entity,
		SecretNames: secretNames,
	}
}

// Implement Identity interface for ServiceAccount
func (sa ServiceAccount) GetToken() string {
	return sa.Token.Raw
}

func (sa ServiceAccount) Can(verb, resource string) bool {
	for _, perm := range sa.Entitelements {
		if perm.Verb == "*" || (perm.Verb == verb && perm.ResourceType == resource) {
			return true
		}
	}
	return false
}

func (sa ServiceAccount) GetEntitlements() []RBACPermission {
	return sa.Entitelements
}

var _ Identity = (*ServiceAccount)(nil)

type Session struct {
	Id          string
	Name        string
	Hostname    string
	Os          string
	Arch        string
	OsVersion   string
	PID         int
	ProcessName string
	User        string
	RemoteAddr  string
	IsRoot      bool
	UID         string
	GID         string
}

// GetId implements Entity.
func (s Session) GetId() string {
	return s.Id
}

// GetKind implements Entity.
func (s Session) GetKind() string {
	return "Session"
}

// GetName implements Entity.
func (s Session) GetName() string {
	return s.Name
}

var _ Entity = (*Session)(nil)

type K8sSecret struct {
	K8sEntity
	// NamespacedResource
	Data map[string]string
	Type string // https://kubernetes.io/docs/concepts/configuration/secret/#secret-types
}

var _ Entity = (*K8sSecret)(nil)

func (s K8sSecret) GetId() string {
	return fmt.Sprintf("ns/%s/secret/%s", s.GetNamespace(), s.GetName())
}
func (s K8sSecret) GetName() string {
	return s.Name
}
func (s K8sSecret) GetKind() string {
	return "Secret"
}

func NewSecretFromK8sSpec(s v1.Secret) K8sSecret {
	entity := NewK8sEntity(s.ObjectMeta.Name, "Secret", s.ObjectMeta.Namespace)

	// Convert map[string][]byte to map[string]string
	dataStr := make(map[string]string, len(s.Data))
	for k, v := range s.Data {
		dataStr[k] = string(v)
	}
	return K8sSecret{
		K8sEntity: entity,
		Type:      string(s.Type),
		Data:      dataStr,
	}
}
