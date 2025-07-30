package campaign

import (
	"fmt"
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
	newFacts, removedFacts, err := analyzeDeployPodFailure(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
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
	newFacts, _, err := analyzeDeployPodFailure(event)
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
		Target: domain.Namespace{Name: nsName},
	}
	newFacts, _, err := analyzeDeployPodFailure(event)
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
	newFacts, _, err := analyzeDeployPodFailure(event)
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
}

func TestAnalyzeDeployPodFailure_UnknownError(t *testing.T) {
	event := domain.TTPExecuted{
		Results: []string{"Some unknown error"},
	}
	newFacts, _, err := analyzeDeployPodFailure(event)
	if err != nil {
		t.Errorf("Expected no error, got %v", err)
	}
	if len(newFacts.Entities) != 0 {
		t.Errorf("Expected no entities, got %v", newFacts.Entities)
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

			newFacts, _, err := analyzeFailedTTPExecution(event)
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

	newFacts, removedFacts, err := analyzeFailedTTPExecution(event)

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
	newFacts, _, err := analyzeFailedTTPExecution(event)
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
		Root:       "/host/path",
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
	// Since the current implementation does not add entities or relations,
	// we just check that it returns empty slices.
	// expect 5 new pods to be found based on their UID
	if len(facts.Entities) != numPods+1 { // +1 for the node entity
		t.Errorf("Expected %d pods exposed through kubelet files , got %v", numPods, facts.Entities)
	}

	// the identified projected SA tokens should be listed as interesting files
	node, ok := facts.Entities[0].(domain.K8sNode)
	if !ok {
		t.Fatalf("Expected first entity to be K8sNode, got %T", facts.Entities[0])
	}
	if len(node.SystemImpl.Files) != numPods {
		t.Errorf("Expected the node to have the %d interesting files, got %d", numPods, len(node.SystemImpl.Files))
	}

	if len(facts.Relations) != 0 {
		t.Errorf("Expected no relations, got %v", facts.Relations)
	}
}
