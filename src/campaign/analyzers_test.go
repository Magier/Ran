package campaign

import (
	"fmt"
	"strings"
	"testing"

	"github.com/Magier/Ran/domain"
)

func TestGetServicesFromEnvVar(t *testing.T) {
	vars := map[string]string{
		"KUBERNETES_PORT":               "tcp://10.96.0.1:443",
		"KUBERNETES_PORT_443_TCP":       "tcp://10.96.0.1:443",
		"KUBERNETES_PORT_443_TCP_ADDR":  "10.96.0.1",
		"KUBERNETES_PORT_443_TCP_PORT":  "443",
		"KUBERNETES_PORT_443_TCP_PROTO": "tcp",
		"KUBERNETES_SERVICE_HOST":       "10.96.0.1",
		"KUBERNETES_SERVICE_PORT":       "443",
		"KUBERNETES_SERVICE_PORT_HTTPS": "443",
		"TRACING_ENABLED":               "true",
		"TRIVY_PORT":                    "tcp://10.96.12.128:4954",
		"TRIVY_PORT_4954_TCP":           "tcp://10.96.12.128:4954",
		"TRIVY_PORT_4954_TCP_ADDR":      "10.96.12.128",
		"TRIVY_PORT_4954_TCP_PORT":      "4954",
		"TRIVY_PORT_4954_TCP_PROTO":     "tcp",
		"TRIVY_SERVICE_HOST":            "10.96.12.128",
		"TRIVY_SERVICE_PORT":            "4954",
		"TRIVY_SERVICE_PORT_TRIVY_HTTP": "4954",
		"MY_SERVER_PORT":                "tcp://10.96.180.142:8080",
		"MY_SERVER_PORT_8080_TCP":       "tcp://10.96.180.142:8080",
		"MY_SERVER_PORT_8080_TCP_ADDR":  "10.96.180.142",
		"MY_SERVER_PORT_8080_TCP_PORT":  "8080",
		"MY_SERVER_PORT_8080_TCP_PROTO": "tcp",
		"MY_SERVER_SERVICE_HOST":        "10.96.180.142",
		"MY_SERVER_SERVICE_PORT":        "8080",
		"MY_SERVER_SERVICE_PORT_GRPC":   "8080",
	}

	services := getServicesFromEnvVars(vars)

	if len(services) != 3 {
		t.Fail()
	}
}
func TestAnalyzeDeployPodFailure_NoResults(t *testing.T) {
	event := domain.TTPExecuted{
		Results: []string{},
	}
	newFacts, removedFacts, _, err := analyzeFailedTTPExecution(event)
	if err == nil {
		t.Errorf("Expected error but got %v", err)
	}
	if len(newFacts.Entities) != 0 {
		t.Errorf("Expected no entities, got %v", newFacts.Entities)
	}
	if len(newFacts.Relations) != 0 {
		t.Errorf("Expected no relations, got %v", newFacts.Relations)
	}
	if len(removedFacts.Entities) != 0 {
		t.Errorf("Expected empty RemovedFacts, got %v", removedFacts)
	}
	if len(removedFacts.Relations) != 0 {
		t.Errorf("Expected empty RemovedFacts, got %v", removedFacts)
	}
}

func TestAnalyzeDeployPodFailure_AlreadyExists(t *testing.T) {
	event := domain.TTPExecuted{
		Results: []string{"Error from server (AlreadyExists): pods \"mypod\" already exists"},
	}
	newFacts, _, _, err := analyzeFailedTTPExecution(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 0 {
		t.Errorf("Expected no entities, got %v", newFacts.Entities)
	}
}

func TestAnalyzeDeployPodFailure_PodSecurityViolation(t *testing.T) {
	nsName := "test-ns"
	event := domain.TTPExecuted{
		Results: []string{
			"command terminated with exit code 1: 'Error from server (Forbidden): error when creating \"STDIN\": pods \"workstation-66549c6f86-vgqch-44183\" is forbidden: violates PodSecurity \"baseline:latest\": hostPath volumes (volume \"hostmount\")\n'",
		},
		Target: domain.NewNamespace(nsName),
	}
	newFacts, _, failReason, err := analyzeFailedTTPExecution(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	ns, ok := newFacts.Entities[0].(domain.Namespace)
	if !ok {
		t.Fatalf("Expected entity to be Namespace, got %T", newFacts.Entities[0])
	}
	if ns.Name != nsName {
		t.Errorf("Expected namespace name '%s', got %s", nsName, ns.Name)
	}
	if ns.EnforcedPSS != "baseline:latest" {
		t.Errorf("Expected EnforcedPSS 'baseline:latest', got %s", ns.EnforcedPSS)
	}
	if !strings.HasPrefix(failReason, "Namespace enforces a PSS") {
		t.Errorf("Expected failReason to be set")
	}
}

func TestAnalyzeDeployPodFailure_PodSecurityViolation_without_target_returns_ns(t *testing.T) {
	nsName := "test-ns"
	event := domain.TTPExecuted{
		Args: map[string]string{
			"Namespace": nsName,
		},
		Results: []string{
			"command terminated with exit code 1: 'Error from server (Forbidden): error when creating \"STDIN\": pods \"workstation-66549c6f86-vgqch-44183\" is forbidden: violates PodSecurity \"baseline:latest\": hostPath volumes (volume \"hostmount\")\n'",
		},
	}
	newFacts, _, failReason, err := analyzeFailedTTPExecution(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	ns, ok := newFacts.Entities[0].(domain.Namespace)
	if !ok {
		t.Fatalf("Expected entity to be Namespace, got %T", newFacts.Entities[0])
	}
	if ns.Name != "test-ns" {
		t.Errorf("Expected namespace name '%s', got %s", nsName, ns.Name)
	}
	if ns.EnforcedPSS != "baseline:latest" {
		t.Errorf("Expected EnforcedPSS 'baseline:latest', got %s", ns.EnforcedPSS)
	}
	if !strings.HasPrefix(failReason, "Namespace enforces a PSS") {
		t.Errorf("Expected failReason to be set")
	}

}

func TestAnalyzeDeployPodFailure_UnknownError(t *testing.T) {
	event := domain.TTPExecuted{
		Results: []string{"Some unknown error"},
	}
	newFacts, _, failReason, err := analyzeFailedTTPExecution(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 0 {
		t.Errorf("Expected no entities, got %v", newFacts.Entities)
	}
	if failReason != "" {
		t.Errorf("Expected failReason to be empty, got %s", failReason)
	}
}
func TestAnalyzeFailedTTPExecution_ToolNotFound(t *testing.T) {
	toolName := "kubectl"
	execSystem := domain.NewPod("mypod", "default")

	tests := []struct {
		name   string
		errMsg string
	}{
		{
			name:   "kubectl not found",
			errMsg: fmt.Sprintf("command terminated with exit code 127: 'sh: 1: %s: not found\n'", toolName),
		},
		{
			name:   "OCI runtime exec failed",
			errMsg: fmt.Sprintf("error: Internal error occurred: Internal error occurred: error executing command in container: failed to exec in container: failed to start exec \"arstarst123\": OCI runtime exec failed: exec failed: unable to start container process: exec: \"%s\": executable file not found in $PATH\n", toolName),
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			event := domain.TTPExecuted{
				Results:    []string{tt.errMsg},
				Procedure:  domain.Procedure{Tool: toolName},
				ExecutedOn: execSystem,
			}

			newFacts, _, _, err := analyzeFailedTTPExecution(event)
			if err != nil {
				t.Errorf("Expected no error, got %v", err)
			}
			if len(newFacts.Entities) != 1 {
				t.Errorf("Expected 1 entity, got %d", len(newFacts.Entities))
			}
			pod, ok := newFacts.Entities[0].(domain.Pod)
			if !ok {
				t.Fatalf("Expected entity to be Pod, got %T", newFacts.Entities[0])
			}
			if val, exists := pod.Binaries[toolName]; !exists || val != "❌" {
				t.Errorf("Expected pod.Binaries[%s]=❌, got %v", toolName, pod.Binaries)
			}
		})
	}
}

func TestAnalyzeFailedTTP_BinaryNotFoundShouldUpdateBinariesOnExecutingSystem(t *testing.T) {
	toolName := "curl"
	event := domain.TTPExecuted{
		Results: []string{
			"Error 127\n",
			"/usr/bin/sh: 1: curl: not found\n",
			"command terminated with exit code 127: '/usr/bin/sh: 1: curl: not found\n'"},
		Procedure: domain.Procedure{
			Tool: toolName,
		},
		ExecutedOn: domain.NewPod("mypod", "default"),
	}

	newFacts, removedFacts, _, err := analyzeFailedTTPExecution(event)

	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 1 {
		t.Errorf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	pod, ok := newFacts.Entities[0].(domain.Pod)
	if !ok {
		t.Fatalf("Expected entity to be Pod, got %T", newFacts.Entities[0])
	}
	if val, exists := pod.Binaries[toolName]; !exists || val != "❌" {
		t.Errorf("Expected pod.Binaries[%s]=❌, got %v", toolName, pod.Binaries)
	}
	if len(removedFacts.Entities) != 0 {
		t.Errorf("Expected no removed entities, got %d", len(removedFacts.Entities))
	}
	if len(removedFacts.Relations) != 0 {
		t.Errorf("Expected no removed relations, got %d", len(removedFacts.Relations))
	}
}

func TestAnalyzeFailedTTPExecution_RBAC_ForbiddenWithUser(t *testing.T) {
	saName := "test-sa"
	ns := "test-ns"
	event := domain.TTPExecuted{
		Results: []string{
			fmt.Sprintf("command terminated with exit code 1: 'Error from server (Forbidden): pods is forbidden: User \"system:serviceaccount:%s:%s\" cannot list resource \"pods\" in API group \"\" in the namespace \"%s\"\n'", ns, saName, ns),
		},
		Procedure: domain.Procedure{Tool: "kubectl"},
		Target:    domain.NewPod("mypod", ns),
	}
	newFacts, _, failReason, err := analyzeFailedTTPExecution(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}

	// expect the returned entity is a service account
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	sa, ok := newFacts.Entities[0].(domain.ServiceAccount)
	if !ok {
		t.Fatalf("Expected entity to be ServiceAccount, got %T", newFacts.Entities[0])
	}
	if sa.Name != saName || sa.Namespace != ns {
		t.Errorf("Expected ServiceAccount name 'default' in namespace '%s', got name '%s' in namespace '%s'", ns, sa.Name, sa.Namespace)
	}

	if len(newFacts.Relations) != 0 {
		t.Errorf("Expected 0 relations, got %d", len(newFacts.Relations))
	}

	if failReason == "" {
		t.Errorf("Expected proper failReason but it was empty")
	}
}
func TestAnalyzeMountInfo_EmptyInput(t *testing.T) {
	pod := domain.NewPod("test-pod", "default")
	pod.Mounts = []domain.Mount{}

	facts, err := analyzeMountInfo(pod)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(facts.Entities) != 0 {
		t.Errorf("Expected no entities, got %v", facts.Entities)
	}
	if len(facts.Relations) != 0 {
		t.Errorf("Expected no relations, got %v", facts.Relations)
	}
}

func TestAnalyzeMountInfo_WithMounts(t *testing.T) {
	// Prepare a mount with some fields
	pod := domain.NewPod("test-pod", "default")
	mount := domain.Mount{
		MountRoot:  "/host/path",
		MountPoint: "/container/path",
		IsHostPath: true,
		Type:       "ext4",
	}
	pod.Mounts = []domain.Mount{mount}

	facts, err := analyzeMountInfo(pod)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	// Since the current implementation does not add entities or relations,
	// we just check that it returns empty slices.
	if len(facts.Entities) != 0 {
		t.Errorf("Expected no entities, got %v", facts.Entities)
	}
	if len(facts.Relations) != 0 {
		t.Errorf("Expected no relations, got %v", facts.Relations)
	}
}

func TestCreatePodFromKubeletMounts(t *testing.T) {
	tests := []struct {
		name     string
		pathStr  string
		uid      string
		hostPath string
		errMsg   string
	}{
		{
			name:    "mountinfo on node itself",
			pathStr: "1740 835 0:129 / /var/lib/kubelet/pods/%s/volumes/kubernetes.io~projected/kube-api-access-ffd8n rw,relatime shared:1070 - tmpfs tmpfs rw,seclabel,size=1993956k,uid=501,gid=1000,inode64",
			uid:     "85986f35-1e64-46d8-b4ac-8fcee502c18f",
			errMsg:  "",
		},
		{
			name:     "mountinfo from pod via hostPath mount",
			pathStr:  "3918 3916 0:217 / /mnt/host/var/lib/kubelet/pods/%s/volumes/kubernetes.io~projected/kube-api-access-8tnjw rw,relatime - tmpfs tmpfs rw,seclabel,size=1993956k,uid=501,gid=1000,inode64",
			hostPath: "/mnt/host",
			uid:      "85986f35-1e64-46d8-b4ac-8fcee502c18f",
			errMsg:   "",
		},
		{
			name:    "mountinfo with no UID should yield error",
			pathStr: "3342 3340 0:396 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw",
			errMsg:  "no pod UID found in mount point: /proc",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var line string
			if tt.uid != "" {
				line = fmt.Sprintf(tt.pathStr, tt.uid)
			} else {
				line = tt.pathStr
			}

			mount, err := parseMountInfoEntry(line)
			if err != nil {
				t.Errorf("Failed to parse mount info: %v", err)
			}
			mount.HostPath = tt.hostPath // this information is supplemented by a more thorough analysis outside of the parser
			pod, err := createPodFromKubeletMounts(mount)
			if tt.errMsg == "" && err != nil {
				t.Errorf("Expected no error, got %v", err)
			} else if tt.errMsg != "" && err == nil {
				t.Errorf("Expected error '%s', got nil", tt.errMsg)
			}

			if pod.UID != tt.uid {
				t.Errorf("Expected pod name '%s', got '%s'", tt.uid, pod.Name)
			}
			if pod.Name != "" {
				t.Errorf("Expected pod name to be empty, got '%s'", pod.Name)
			}
			if pod.Namespace != "" {
				t.Errorf("Expected pod namespace to be empty, got '%s'", pod.Name)
			}
		})
	}
}

func TestAnalyzeMountInfo_WithHostKubeletInfos(t *testing.T) {
	// podUIDs := []string{
	// 	"85986f35-1e64-46d8-b4ac-8fcee502c18f",
	// 	"6d259dfe-4227-4cad-958c-da7d44cb1daa",
	// 	"8238b82c-8ba1-454f-9508-d2bf78699c74",
	// 	"1b2033a4-2bf7-4e6f-acda-d2ac8a000d9d",
	// 	"69defcbb-7483-41f0-8690-19729e42863a",
	// }
	// projectedSATokenPaths := []string{
	// 	"/var/lib/kubelet/pods/85986f35-1e64-46d8-b4ac-8fcee502c18f/volumes/kubernetes.io~projected/kube-api-access-xbvm8",
	// 	"/var/lib/kubelet/pods/6d259dfe-4227-4cad-958c-da7d44cb1daa/volumes/kubernetes.io~projected/kube-api-access-t8v24",
	// 	"/var/lib/kubelet/pods/8238b82c-8ba1-454f-9508-d2bf78699c74/volumes/kubernetes.io~projected/kube-api-access-8tnjw",
	// 	"/var/lib/kubelet/pods/1b2033a4-2bf7-4e6f-acda-d2ac8a000d9d/volumes/kubernetes.io~projected/kube-api-access-rzw7k",
	// 	"/var/lib/kubelet/pods/69defcbb-7483-41f0-8690-19729e42863a/volumes/kubernetes.io~projected/kube-api-access-5j2q4",
	// }

	mountInfoStrings := []string{
		"3906 2769 0:421 / / rw,relatime - overlay overlay rw,seclabel,lowerdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/38/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/37/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/36/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/35/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/34/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/33/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/32/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/31/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/30/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/29/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/28/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/27/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/26/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/25/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/24/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/23/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/22/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/20/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/19/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/18/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/17/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/16/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/15/fs,upperdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/50011/fs,workdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/50011/work,redirect_dir=nofollow,uuid=on,userxattr",
		`3914 3906 0:62 / /mnt/host rw,relatime - overlay overlay rw,context="system_u:object_r:container_file_t:s0:c1022,c1023",lowerdir=/var/home/core/.local/share/containers/storage/overlay/l/BTSOOTPTOH25C6MZDKKNFPDMU5:/var/home/core/.local/share/containers/storage/overlay/l/MZFSHHTOAWWQFH3T7ZBF27FOYN,upperdir=/var/home/core/.local/share/containers/storage/overlay/c52cea6c189a194dd5772cd95895bd0b49262459e81aadd53fd071be5f616a15/diff,workdir=/var/home/core/.local/share/containers/storage/overlay/c52cea6c189a194dd5772cd95895bd0b49262459e81aadd53fd071be5f616a15/work,redirect_dir=nofollow,userxattr`,
		"3916 3914 252:4 /ostree/deploy/fedora-coreos/var/home/core/.local/share/containers/storage/volumes/443579c837e6de9d2b28c81ff56328e90c168aa99c4f7afa741978c93f4417c0/_data /mnt/host/var rw,relatime - xfs /dev/vda4 rw,seclabel,attr2,inode64,logbufs=8,logbsize=32k,prjquota",
		"3917 3916 0:261 / /mnt/host/var/lib/kubelet/pods/85986f35-1e64-46d8-b4ac-8fcee502c18f/volumes/kubernetes.io~projected/kube-api-access-xbvm8 rw,relatime - tmpfs tmpfs rw,seclabel,size=1993956k,uid=501,gid=1000,inode64",
		"3917 3916 0:216 / /mnt/host/var/lib/kubelet/pods/6d259dfe-4227-4cad-958c-da7d44cb1daa/volumes/kubernetes.io~projected/kube-api-access-t8v24 rw,relatime - tmpfs tmpfs rw,seclabel,size=1993956k,uid=501,gid=1000,inode64",
		"3918 3916 0:217 / /mnt/host/var/lib/kubelet/pods/8238b82c-8ba1-454f-9508-d2bf78699c74/volumes/kubernetes.io~projected/kube-api-access-8tnjw rw,relatime - tmpfs tmpfs rw,seclabel,size=1993956k,uid=501,gid=1000,inode64",
		"3919 3916 0:225 / /mnt/host/var/lib/kubelet/pods/1b2033a4-2bf7-4e6f-acda-d2ac8a000d9d/volumes/kubernetes.io~projected/kube-api-access-rzw7k rw,relatime - tmpfs tmpfs rw,seclabel,size=1993956k,uid=501,gid=1000,inode64",
		"3920 3916 0:232 / /mnt/host/var/lib/kubelet/pods/69defcbb-7483-41f0-8690-19729e42863a/volumes/kubernetes.io~projected/kube-api-access-bjzfp rw,relatime - tmpfs tmpfs rw,seclabel,size=1993956k,uid=501,gid=1000,inode64",
	}

	mounts := make([]domain.Mount, len(mountInfoStrings))
	for i, mountInfo := range mountInfoStrings {
		mount, err := parseMountInfoEntry(mountInfo)
		if err != nil {
			t.Errorf("Failed to parse mount info '%s': %v", mountInfo, err)
			return
		}
		mounts[i] = mount
	}

	pod := domain.NewPod("test-pod", "default")
	pod.Mounts = mounts
	numPods := 5

	facts, err := analyzeMountInfo(pod)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	// +2 for the node and redundant srcPod (don't have information to merge UID and srcPod at this point)
	if len(facts.Entities) != numPods+2 {
		t.Errorf("Expected %d pods exposed through kubelet files , got %v", numPods, facts.Entities)
	}

	nodeFound := false
	// the identified projected SA tokens should be listed as interesting files
	for _, e := range facts.Entities {
		if node, ok := e.(domain.K8sNode); ok {
			nodeFound = true
			if len(node.SystemImpl.Files) != numPods {
				t.Errorf("Expected the node to have the %d interesting files, got %d", numPods, len(node.SystemImpl.Files))
			}
			break
		}
	}

	if !nodeFound {
		t.Errorf("Expected to find a K8sNode entity in the facts, got %v", facts.Entities)
	}

	if len(facts.Relations) != numPods+1 { // +1 for the (redundant) srcPod
		t.Errorf("Expected a runs-on relation for every pod link to a node %v", facts.Relations)
	}
}

func TestAnalyzeUnknownSystem_MatchesToExistingPod(t *testing.T) {
	// Create a campaign
	c := NewCampaign(nil)

	// Create a known pod with a specific hostname
	hostName := "pod-657596d964-rp5kz"
	namespace := "default"
	knownPod := domain.NewPod(hostName, namespace)
	knownPod.SystemImpl.HostName = hostName

	// Create an unknown system with the same hostname but additional information
	unknownSys := domain.NewSystem(hostName, "Linux", domain.RootExec)
	unknownSys.Binaries = map[string]string{
		"curl": "✓",
		"wget": "✓",
	}

	// Set up the test with the existing pod in the campaign's known entities
	// This would typically be done through another mechanism, but for testing
	// we'll inject it directly
	c.AddEntities(knownPod)

	// Analyze the unknown system
	newFacts, removedFacts, err := c.analyzeUnknownSystem(unknownSys)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}

	// Should return the matched pod with merged information
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}

	// The entity should be a Pod with the same ID as our known Pod
	updatedPod, ok := newFacts.Entities[0].(domain.Pod)
	if !ok {
		t.Fatalf("Expected entity to be a Pod, got %T", newFacts.Entities[0])
	}

	// Verify identity is preserved
	if updatedPod.Name != hostName || updatedPod.Namespace != namespace {
		t.Errorf("Expected pod name=%s namespace=%s, got name=%s namespace=%s",
			hostName, namespace, updatedPod.Name, updatedPod.Namespace)
	}

	// Verify original data is preserved
	if updatedPod.OS != "Linux" {
		t.Errorf("Expected OS to remain 'Linux', got '%s'", updatedPod.OS)
	}

	if len(updatedPod.Binaries) != len(unknownSys.Binaries) {
		t.Errorf("Expected original binaries to be preserved, got %v", updatedPod.Binaries)
	}
	for k, v := range unknownSys.Binaries {
		if updatedPod.Binaries[k] != v {
			t.Errorf("Expected original binary '%s' to be preserved, got %v", k, updatedPod.Binaries)
		}
	}

	// Verify new data is merged
	if updatedPod.Binaries["curl"] != "✓" || updatedPod.Binaries["wget"] != "✓" {
		t.Errorf("Expected new binaries to be added, got %v", updatedPod.Binaries)
	}
	// if len(updatedPod.Files) != 1 || updatedPod.Files[0].Path != "/etc/passwd" {
	// 	t.Errorf("Expected files to be merged, got %v", updatedPod.Files)
	// }

	// No entities should be removed
	if len(removedFacts.Entities) != 0 {
		t.Errorf("Expected no removed entities, got %d", len(removedFacts.Entities))
	}
}

func TestAnalyzeDnsEntriesScan(t *testing.T) {
	tests := []struct {
		name         string
		ip           string
		dns          string
		expectedKind string
		expectedName string
		expectedNS   string
		expectError  bool
	}{
		{
			name:         "standard pod DNS",
			ip:           "192.168.1.4",
			dns:          "192-168-1-4.backend-service.dev.svc.cluster.local",
			expectedKind: "Pod",
			expectedName: "backend-service",
			expectedNS:   "dev",
			expectError:  false,
		},
		{
			name:         "ClusterIP service DNS",
			ip:           "10.96.5.1.",
			dns:          "backend-service.dev.svc.cluster.local",
			expectedKind: "Pod",
			expectedName: "backend-service",
			expectedNS:   "dev",
			expectError:  false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			newFacts, _, err := analyzeDnsEntries(map[string]string{tt.ip: tt.dns})
			if tt.expectError {
				if err == nil {
					t.Errorf("Expected error but got none")
				}
				return
			} else {
				if err != nil {
					t.Errorf("Expected no error but got %v", err)
					return
				}
			}

			if len(newFacts.Entities) != 1 {
				t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
			}

			entity := newFacts.Entities[0]

			if k := entity.GetKind(); k != tt.expectedKind {
				t.Errorf("Expected entity kind '%s', got '%s'", tt.expectedKind, k)
			}
			if name := entity.GetName(); name != tt.expectedName {
				t.Errorf("Expected entity name '%s', got '%s'", tt.expectedName, name)
			}

			if tt.expectedKind == "Pod" {
				_, ok := entity.(domain.Pod)
				if !ok {
					t.Fatalf("Expected entity to be Pod, got %T", entity)
				}
			} else {
				_, ok := entity.(domain.Service)
				if !ok {
					t.Fatalf("Expected entity to be Service, got %T", entity)
				}
			}

			if nsEntity, ok := entity.(domain.Namespaced); ok {
				ns := nsEntity.GetNamespace()
				if ns != tt.expectedNS {
					t.Errorf("Expected entity namespace '%s', got '%s'", tt.expectedNS, ns)
				}
			}

			// pod, ok := newFacts.Entities[0].(domain.Pod)
			// if !ok {
			// 	t.Fatalf("Expected entity to be Pod, got %T", newFacts.Entities[0])
			// }
			// if pod.Name != tt.expectedName {
			// 	t.Errorf("Expected entity name '%s', got '%s'", tt.expectedName, pod.Name)
			// }

			// if pod.Namespace != tt.expectedNS {
			// 	t.Errorf("Expected entity namespace '%s', got '%s'", tt.expectedNS, pod.Namespace)
			// }
		})
	}
}
