package campaign

import (
	"testing"

	"github.com/Magier/Ran/domain"
)

func TestSelfSubjectReviewResult_ForbiddenStatus(t *testing.T) {
	res := `{
		"kind": "Status",
		"apiVersion": "v1",
		"metadata": {},
		"status": "Failure",
		"message": "selfsubjectrulesreviews.authorization.k8s.io is forbidden: User \"system:anonymous\" cannot create resource \"selfsubjectrulesreviews\" in API group \"authorization.k8s.io\" at the cluster scope",
		"reason": "Forbidden",
		"details": {
		  "group": "authorization.k8s.io",
		  "kind": "selfsubjectrulesreviews"
		},
		"code": 403
	}`
	var _ = res

	// Genereated stuff:
	// ev := domain.TTPExecuted{}
	// source := domain.Entity{}
	// args := []string{"", "Forbidden"}

	// result, err := HandleSelfSubjectReviewResult(ev, source, args...)
	// if err != nil {
	// 	t.Fatalf("Expected no error, got: %v", err)
	// }

	// expected := domain.SelfSubjectReviewResult{
	// 	Source: source,
	// 	Status: "Forbidden",
	// }

	//	if result != expected {
	//		t.Fatalf("Expected %v, got: %v", expected, result)
	//	}
}
func TestParseEffect_TargetIP(t *testing.T) {
	source := domain.NewPod("mypod", "myns")
	args := map[string]string{}
	results := []string{"10.0.0.1 10.0.0.2"}
	newFacts, removedFacts := ParseEffect("target.ip", source, args, results...)
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	pod, ok := newFacts.Entities[0].(domain.Pod)
	if !ok {
		t.Fatalf("Expected entity to be Pod")
	}
	if len(pod.IPs) != 2 {
		t.Fatalf("Expected 2 IPs, got %d", len(pod.IPs))
	}
	if pod.IPs[0].IP.String() != "10.0.0.1" || pod.IPs[1].IP.String() != "10.0.0.2" {
		t.Fatalf("Unexpected IPs: %+v", pod.IPs)
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_K8sPodList(t *testing.T) {
	// minimal pod list json
	results := []string{`{"items":[{"metadata":{"name":"pod1","namespace":"ns1"}},{"metadata":{"name":"pod2","namespace":"ns2"}}]}`}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("k8s.podlist", source, args, results...)
	if len(newFacts.Entities) != 2 {
		t.Fatalf("Expected 2 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_K8sDeploymentList(t *testing.T) {
	results := []string{`{"items":[{"metadata":{"name":"dep1","namespace":"ns1"}},{"metadata":{"name":"dep2","namespace":"ns2"}}]}`}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("k8s.deploymentlist", source, args, results...)
	if len(newFacts.Entities) != 2 {
		t.Fatalf("Expected 2 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_K8sServiceAccountList(t *testing.T) {
	results := []string{`{"items":[{"metadata":{"name":"sa1","namespace":"ns1"}},{"metadata":{"name":"sa2","namespace":"ns2"}}]}`}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("k8s.serviceaccountlist", source, args, results...)
	if len(newFacts.Entities) != 2 {
		t.Fatalf("Expected 2 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_K8sServiceAccount_Created(t *testing.T) {
	results := []string{"serviceaccount/my-sa created"}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{"Name": "my-sa", "Namespace": "ns1"}
	newFacts, removedFacts := ParseEffect("k8s.serviceaccount", source, args, results...)
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	sa, ok := newFacts.Entities[0].(domain.ServiceAccount)
	if !ok {
		t.Fatalf("Expected entity to be ServiceAccount")
	}
	if sa.GetName() != "my-sa" || sa.GetNamespace() != "ns1" {
		t.Fatalf("Unexpected ServiceAccount: %+v", sa)
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_K8sServiceAccount_AlreadyExists(t *testing.T) {
	results := []string{"Error: already exists"}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{"Name": "my-sa", "Namespace": "ns1"}
	newFacts, removedFacts := ParseEffect("k8s.serviceaccount", source, args, results...)
	if len(newFacts.Entities) != 1 {
		t.Fatalf("Expected 1 entity, got %d", len(newFacts.Entities))
	}
	sa, ok := newFacts.Entities[0].(domain.ServiceAccount)
	if !ok {
		t.Fatalf("Expected entity to be ServiceAccount")
	}
	if sa.GetName() != "my-sa" || sa.GetNamespace() != "ns1" {
		t.Fatalf("Unexpected ServiceAccount: %+v", sa)
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_DeleteK8sServiceAccount(t *testing.T) {
	results := []string{"serviceaccount/my-sa deleted"}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{"Name": "my-sa", "Namespace": "ns1"}
	newFacts, removedFacts := ParseEffect("delete k8s.serviceaccount", source, args, results...)
	if len(removedFacts.Entities) != 1 {
		t.Fatalf("Expected 1 removed entity, got %d", len(removedFacts.Entities))
	}
	sa, ok := removedFacts.Entities[0].(domain.ServiceAccount)
	if !ok {
		t.Fatalf("Expected removed entity to be ServiceAccount")
	}
	if sa.GetName() != "my-sa" || sa.GetNamespace() != "ns1" {
		t.Fatalf("Unexpected ServiceAccount: %+v", sa)
	}
	if len(newFacts.Entities) != 0 {
		t.Fatalf("Expected 0 new entities")
	}
}

func TestParseEffect_DeleteK8sPod(t *testing.T) {
	results := []string{"irrelevant"}
	ns := "test-ns"
	name := "mypod"
	source := domain.NewPod(name, ns)
	args := map[string]string{"Name": name, "Namespace": ns}
	newFacts, removedFacts := ParseEffect("delete k8s.pod", source, args, results...)
	if len(removedFacts.Entities) != 1 {
		t.Fatalf("Expected 1 removed entity, got %d", len(removedFacts.Entities))
	}
	pod, ok := removedFacts.Entities[0].(domain.Pod)
	if !ok {
		t.Fatalf("Expected removed entity to be Pod")
	}
	if pod.GetName() != name || pod.GetNamespace() != ns {
		t.Fatalf("Unexpected Pod: %+v", pod)
	}
	if len(newFacts.Entities) != 0 {
		t.Fatalf("Expected 0 new entities")
	}
}

func TestParseEffect_DeleteK8sDeployment(t *testing.T) {
	results := []string{"irrelevant"}
	ns := "test-ns"
	name := "mydeployment"
	source := domain.NewDeployment(name, ns)
	args := map[string]string{"Name": name, "Namespace": ns}
	newFacts, removedFacts := ParseEffect("delete k8s.deployment", source, args, results...)
	if len(removedFacts.Entities) != 1 {
		t.Fatalf("Expected 1 removed entity, got %d", len(removedFacts.Entities))
	}
	dep, ok := removedFacts.Entities[0].(domain.Deployment)
	if !ok {
		t.Fatalf("Expected removed entity to be Deployment")
	}
	if dep.GetName() != name || dep.GetNamespace() != ns {
		t.Fatalf("Unexpected Deployment: %+v", dep)
	}
	if len(newFacts.Entities) != 0 {
		t.Fatalf("Expected 0 new entities")
	}
}

func TestParseEffect_K8sSecretList(t *testing.T) {
	results := []string{`{"items":[{"metadata":{"name":"secret1","namespace":"ns1"}},{"metadata":{"name":"secret2","namespace":"ns2"}}]}`}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("k8s.secretlist", source, args, results...)
	if len(newFacts.Entities) != 2 {
		t.Fatalf("Expected 2 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_UnknownEffect(t *testing.T) {
	results := []string{"irrelevant"}
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("unknown.effect", source, args, results...)
	if len(newFacts.Entities) != 0 {
		t.Fatalf("Expected 0 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}

func TestParseEffect_NoResults(t *testing.T) {
	source := domain.NewPod("irrelevant", "irrelevant")
	args := map[string]string{}
	newFacts, removedFacts := ParseEffect("k8s.podlist", source, args)
	if len(newFacts.Entities) != 0 {
		t.Fatalf("Expected 0 entities, got %d", len(newFacts.Entities))
	}
	if len(removedFacts.Entities) != 0 {
		t.Fatalf("Expected 0 removed entities")
	}
}
func Test_parsePrettySelfSubjectRulesReview(t *testing.T) {
	input := `Resources                                       Non-Resource URLs                      Resource Names   Verbs
*.*                                             []                                     []               [*]
*                                               []                                     []               [*]
selfsubjectreviews.authentication.k8s.io        []                                     []               [create]
selfsubjectaccessreviews.authorization.k8s.io   []                                     []               [create]
selfsubjectrulesreviews.authorization.k8s.io    []                                     []               [create]
rolebindings.rbac.authorization.k8s.io          []                                     []               [create]
roles.rbac.authorization.k8s.io                 []                                     []               [create]
pods/exec                                       []                                     []               [get create]
pods/log                                        []                                     []               [get create]
pods                                            []                                     []               [get list create delete]
events                                          []                                     []               [get list]
namespaces                                      []                                     []               [get list]
serviceaccounts                                 []                                     []               [get list]
deployments.apps                                []                                     []               [get list]
replicasets.apps                                []                                     []               [get list]
												[/.well-known/openid-configuration/]   []               [get]
												[/.well-known/openid-configuration]    []               [get]
												[/api/*]                               []               [get]
												[/api]                                 []               [get]
												[/apis/*]                              []               [get]
												[/apis]                                []               [get]
												[/healthz]                             []               [get]
												[/healthz]                             []               [get]
												[/livez]                               []               [get]
												[/livez]                               []               [get]
												[/openapi/*]                           []               [get]
												[/openapi]                             []               [get]
												[/openid/v1/jwks/]                     []               [get]
												[/openid/v1/jwks]                      []               [get]
												[/readyz]                              []               [get]
												[/readyz]                              []               [get]
												[/version/]                            []               [get]
												[/version/]                            []               [get]
												[/version]                             []               [get]
												[/version]                             []               [get]`

	result, err := parsePrettySelfSubjectRulesReview(input)
	if err != nil {
		t.Fatalf("Expected no error, got: %v", err)
	}

	// the JSON equivalent returns 3 NonResourceRules and 11 ResourceRules
	// the grouping of the rules itself is not reproducable from the data itself,
	// so this tests, that the parsing returns at least the same number of (ungrouped) rules, as the grouped JSON counterpart
	// 3 are returned by the JSON equivalent
	numNonResourceRules := len(result.Status.NonResourceRules)
	if numNonResourceRules != 20 {
		t.Fatalf("Expected at least 3 NonResourceRules, got: %d", len(result.Status.NonResourceRules))
	}

	// 11 are returned by the JSON equivalent
	numResourceRules := len(result.Status.ResourceRules)
	if numResourceRules != 15 {
		t.Fatalf("Expected at 15 ResourceRules, got: %d", len(result.Status.ResourceRules))
	}
}

func Test_inferLinuxMountFormat(t *testing.T) {
	tests := map[string]struct {
		line       string
		parserName string
		parserFn   MountEntryParserFn
	}{
		"unknwon format": {
			line:     "this is not a valid mount info line",
			parserFn: nil,
		},
		"mountinfo": {
			line:       "3737 3709 0:446 / /dev rw,nosuid - tmpfs tmpfs rw,seclabel,size=65536k,mode=755,uid=501,gid=1000,inode64",
			parserName: "parseMountInfoEntry",
			parserFn:   parseMountInfoEntry,
		},
		"mount command": {
			line:       `overlay on /host type overlay (rw,relatime,context="system_u:object_r:container_file_t:s0:c1022,c1023",lowerdir=/var/home/core/.local/share/containers/storage/overlay/l/BTSOOTPTOH25C6MZDKKNFPDMU5:/var/home/core/.local/share/containers/storage/overlay/l/MZFSHHTOAWWQFH3T7ZBF27FOYN,upperdir=/var/home/core/.local/share/containers/storage/overlay/c52cea6c189a194dd5772cd95895bd0b49262459e81aadd53fd071be5f616a15/diff,workdir=/var/home/core/.local/share/containers/storage/overlay/c52cea6c189a194dd5772cd95895bd0b49262459e81aadd53fd071be5f616a15/work,redirect_dir=nofollow,uuid=on,userxattr)`,
			parserName: "parseMountCommandEntry",
			parserFn:   parseMountCommandEntry,
		},
		"proc/self/mount": {
			line:       `tmpfs /dev tmpfs rw,seclabel,nosuid,size=65536k,mode=755,uid=501,gid=1000,inode64 0 0`,
			parserName: "parserProcMountEntry",
			parserFn:   parseProcMountEntry,
		},
	}

	for name, test := range tests {
		t.Run(name, func(t *testing.T) {
			fn := getMountEntryParser(test.line)
			if (fn == nil && test.parserFn != nil) || (fn != nil && test.parserFn == nil) {
				t.Errorf("Expected parserFn %s (%v), got %v", test.parserName, test.parserFn, fn)
			}
		})
	}
}

func Test_parseLinuxMountInfo_NoAdditionalVolumeMounts(t *testing.T) {
	data := `3709 3150 0:400 / / rw,relatime - overlay overlay rw,seclabel,lowerdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21044/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21043/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21042/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21041/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21040/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21039/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21038/fs,upperdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/48990/fs,workdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/48990/work,redirect_dir=nofollow,uuid=on,userxattr
3713 3709 0:444 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw
3737 3709 0:446 / /dev rw,nosuid - tmpfs tmpfs rw,seclabel,size=65536k,mode=755,uid=501,gid=1000,inode64
3739 3737 0:448 / /dev/pts rw,nosuid,noexec,relatime - devpts devpts rw,seclabel,gid=100004,mode=620,ptmxmode=666
3740 3737 0:418 / /dev/mqueue rw,nosuid,nodev,noexec,relatime - mqueue mqueue rw,seclabel
3741 3709 0:427 / /sys ro,nosuid,nodev,noexec,relatime - sysfs sysfs ro,seclabel
3872 3741 0:29 / /sys/fs/cgroup ro,nosuid,nodev,noexec,relatime - cgroup2 cgroup rw,seclabel,nsdelegate,memory_recursiveprot
3873 3709 252:4 /ostree/deploy/fedora-coreos/var/home/core/.local/share/containers/storage/volumes/443579c837e6de9d2b28c81ff56328e90c168aa99c4f7afa741978c93f4417c0/_data/lib/kubelet/pods/6b2441c4-600b-4c24-95d4-ec2c1d63088b/etc-hosts /etc/hosts rw,relatime - xfs /dev/vda4 rw,seclabel,attr2,inode64,logbufs=8,logbsize=32k,prjquota
3874 3737 252:4 /ostree/deploy/fedora-coreos/var/home/core/.local/share/containers/storage/volumes/443579c837e6de9d2b28c81ff56328e90c168aa99c4f7afa741978c93f4417c0/_data/lib/kubelet/pods/6b2441c4-600b-4c24-95d4-ec2c1d63088b/containers/backend/8504a7c5 /dev/termination-log rw,relatime - xfs /dev/vda4 rw,seclabel,attr2,inode64,logbufs=8,logbsize=32k,prjquota
3875 3709 252:4 /ostree/deploy/fedora-coreos/var/home/core/.local/share/containers/storage/volumes/443579c837e6de9d2b28c81ff56328e90c168aa99c4f7afa741978c93f4417c0/_data/lib/containerd/io.containerd.grpc.v1.cri/sandboxes/2766762cb4403108e0ed299340a98d82810187db9ae4c675dfedba0500bf4bfc/hostname /etc/hostname rw,relatime - xfs /dev/vda4 rw,seclabel,attr2,inode64,logbufs=8,logbsize=32k,prjquota
3876 3709 252:4 /ostree/deploy/fedora-coreos/var/home/core/.local/share/containers/storage/volumes/443579c837e6de9d2b28c81ff56328e90c168aa99c4f7afa741978c93f4417c0/_data/lib/containerd/io.containerd.grpc.v1.cri/sandboxes/2766762cb4403108e0ed299340a98d82810187db9ae4c675dfedba0500bf4bfc/resolv.conf /etc/resolv.conf rw,relatime - xfs /dev/vda4 rw,seclabel,attr2,inode64,logbufs=8,logbsize=32k,prjquota
3877 3737 0:379 / /dev/shm rw,relatime - tmpfs shm rw,seclabel,size=65536k,uid=501,gid=1000,inode64
3878 3709 0:391 / /run/secrets/kubernetes.io/serviceaccount ro,relatime - tmpfs tmpfs rw,seclabel,size=1993956k,uid=501,gid=1000,inode64
3879 3737 0:6 /null /dev/null rw,nosuid,noexec master:574 - devtmpfs devtmpfs rw,seclabel,size=4096k,nr_inodes=229545,mode=755,inode64
3880 3737 0:6 /random /dev/random rw,nosuid,noexec master:579 - devtmpfs devtmpfs rw,seclabel,size=4096k,nr_inodes=229545,mode=755,inode64
3881 3737 0:6 /full /dev/full rw,nosuid,noexec master:664 - devtmpfs devtmpfs rw,seclabel,size=4096k,nr_inodes=229545,mode=755,inode64
3882 3737 0:6 /tty /dev/tty rw,nosuid,noexec master:675 - devtmpfs devtmpfs rw,seclabel,size=4096k,nr_inodes=229545,mode=755,inode64
3883 3737 0:6 /zero /dev/zero rw,nosuid,noexec master:657 - devtmpfs devtmpfs rw,seclabel,size=4096k,nr_inodes=229545,mode=755,inode64
3884 3737 0:6 /urandom /dev/urandom rw,nosuid,noexec master:562 - devtmpfs devtmpfs rw,seclabel,size=4096k,nr_inodes=229545,mode=755,inode64
3885 3741 0:400 /product_name /sys/devices/virtual/dmi/id/product_name ro,relatime - overlay overlay rw,seclabel,lowerdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21044/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21043/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21042/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21041/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21040/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21039/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21038/fs,upperdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/48990/fs,workdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/48990/work,redirect_dir=nofollow,uuid=on,userxattr
3886 3741 0:400 /product_uuid /sys/devices/virtual/dmi/id/product_uuid ro,relatime - overlay overlay rw,seclabel,lowerdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21044/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21043/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21042/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21041/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21040/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21039/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21038/fs,upperdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/48990/fs,workdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/48990/work,redirect_dir=nofollow,uuid=on,userxattr
3887 3886 0:400 /product_uuid /sys/devices/virtual/dmi/id/product_uuid ro,relatime - overlay overlay rw,seclabel,lowerdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21044/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21043/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21042/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21041/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21040/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21039/fs:/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/21038/fs,upperdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/48990/fs,workdir=/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/48990/work,redirect_dir=nofollow,uuid=on,userxattr
3151 3713 0:444 /bus /proc/bus ro,nosuid,nodev,noexec,relatime - proc proc rw
3152 3713 0:444 /fs /proc/fs ro,nosuid,nodev,noexec,relatime - proc proc rw
3153 3713 0:444 /irq /proc/irq ro,nosuid,nodev,noexec,relatime - proc proc rw
3154 3713 0:444 /sys /proc/sys ro,nosuid,nodev,noexec,relatime - proc proc rw
3155 3713 0:444 /sysrq-trigger /proc/sysrq-trigger ro,nosuid,nodev,noexec,relatime - proc proc rw
3156 3713 0:459 / /proc/acpi ro,relatime - tmpfs tmpfs ro,seclabel,uid=501,gid=1000,inode64
3157 3713 0:6 /null /proc/kcore rw,nosuid,noexec master:574 - devtmpfs devtmpfs rw,seclabel,size=4096k,nr_inodes=229545,mode=755,inode64
3158 3713 0:6 /null /proc/keys rw,nosuid,noexec master:574 - devtmpfs devtmpfs rw,seclabel,size=4096k,nr_inodes=229545,mode=755,inode64
3159 3713 0:6 /null /proc/latency_stats rw,nosuid,noexec master:574 - devtmpfs devtmpfs rw,seclabel,size=4096k,nr_inodes=229545,mode=755,inode64
3243 3713 0:6 /null /proc/timer_list rw,nosuid,noexec master:574 - devtmpfs devtmpfs rw,seclabel,size=4096k,nr_inodes=229545,mode=755,inode64
3244 3713 0:460 / /proc/scsi ro,relatime - tmpfs tmpfs ro,seclabel,uid=501,gid=1000,inode64
3245 3741 0:467 / /sys/firmware ro,relatime - tmpfs tmpfs ro,seclabel,uid=501,gid=1000,inode64`
	mounts, err := parseLinuxMounts(data)
	if err != nil {
		t.Fatalf("Expected no error, got: %v", err)
	}
	if len(mounts) != 34 {
		t.Fatalf("Expected 34 mounts, got %d", len(mounts))
	}
}

func Test_parseLinuxMounts_InvalidLine(t *testing.T) {
	data := "this is not a valid mount line"
	_, err := parseLinuxMounts(data)
	if err == nil {
		t.Fatalf("Expected error for invalid mount line, got nil")
	}
}

func Test_parseLinuxMounts_EmptyInput(t *testing.T) {
	data := ""
	mounts, err := parseLinuxMounts(data)
	if err == nil {
		t.Fatalf("Expected error for empty input")
	}
	if len(mounts) != 0 {
		t.Fatalf("Expected 0 mounts for empty input, got %d", len(mounts))
	}
}
func Test_parseHasBinaryEffect(t *testing.T) {
	type testCase struct {
		name        string
		source      domain.Entity
		effect      string
		args        map[string]string
		results     []string
		expectError bool
		expectBin   string
		expectPath  string
	}

	tests := []testCase{
		{
			name:        "Valid effect with binary arg",
			source:      domain.NewPod("mypod", "ns"),
			effect:      "target.has-binary(${BINARY_NAME})",
			args:        map[string]string{"BINARY_NAME": "bash"},
			results:     []string{},
			expectError: false,
			expectBin:   "bash",
			expectPath:  "bash",
		},
		{
			name:        "Missing binary arg",
			source:      domain.NewPod("mypod", "ns"),
			effect:      "target.has-binary(${BINARY_NAME})",
			args:        map[string]string{},
			results:     []string{},
			expectError: false, // function does not return error, just warns
			expectBin:   "",
		},
		{
			name:        "Effect string does not match pattern",
			source:      domain.NewPod("mypod", "ns"),
			effect:      "target.has-binary",
			args:        map[string]string{"BINARY_NAME": "bash"},
			results:     []string{},
			expectError: true,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			entity, err := parseHasBinaryEffect(tc.source, tc.effect, tc.args, tc.results...)
			if tc.expectError {
				if err == nil {
					t.Fatalf("Expected error but got nil")
				}
				return
			}
			if err != nil && !tc.expectError {
				t.Fatalf("Unexpected error: %v", err)
			}
			if tc.expectBin != "" {
				pod, ok := entity.(domain.Pod)
				if !ok {
					t.Fatalf("Expected entity to be Pod, got %T", entity)
				}
				val, ok := pod.Binaries[tc.expectBin]
				if !ok {
					t.Fatalf("Expected binary %q in pod.Binaries", tc.expectBin)
				}
				if val != tc.expectPath {
					t.Fatalf("Expected binary path %q, got %q", tc.expectPath, val)
				}
			}
		})
	}
}
func Test_parseLinuxProcesses(t *testing.T) {
	data := `UID   PID    PPID  C STIME TTY          TIME CMD
root           1       0  0 Jul31 ?        00:00:00 /usr/sbin/sshd -D -p 3456 -e
root         649       1  0 20:28 pts/0    00:00:00 /usr/bin/bash`

	// Valid input: two process lines
	procs, err := parseLinuxProcesses(data)
	if err != nil {
		t.Fatalf("Expected no error, got: %v", err)
	}
	if len(procs) != 2 {
		t.Fatalf("Expected 2 processes, got %d", len(procs))
	}
	if procs[0].PID != 1 || procs[0].ParentPID != 0 || procs[0].Cmd != "/usr/sbin/sshd -D -p 3456 -e" {
		t.Errorf("Unexpected first process: %+v", procs[0])
	}
	if procs[1].PID != 649 || procs[1].ParentPID != 1 || procs[1].Cmd != "/usr/bin/bash" {
		t.Errorf("Unexpected second process: %+v", procs[1])
	}

	// Invalid input: missing fields
	data = `1`
	_, err = parseLinuxProcesses(data)
	if err == nil {
		t.Fatalf("Expected error for invalid process line, got nil")
	}

	// Empty input
	data = ""
	_, err = parseLinuxProcesses(data)
	if err == nil {
		t.Fatalf("Expected error for empty input")
	}
}
