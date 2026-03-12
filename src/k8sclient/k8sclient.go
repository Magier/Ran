package k8s

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"log/slog"
	"net"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"time"

	"encoding/json"

	"github.com/Magier/Ran/domain"
	appsV1 "k8s.io/api/apps/v1"
	v1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/kubernetes/scheme"
	"k8s.io/client-go/rest"
	restclient "k8s.io/client-go/rest"
	"k8s.io/client-go/tools/clientcmd"
	"k8s.io/client-go/tools/remotecommand"
	k8s_exec "k8s.io/client-go/util/exec"
)

type ExecError = k8s_exec.CodeExitError

type KubeContext struct {
	Name     string
	UserCert []uint8
	UserKey  []uint8
	AuthExec bool
	Server   string
	ServerCA []uint8
}

func GetConfig() (*restclient.Config, KubeContext, error) {
	home, exists := os.LookupEnv("HOME")
	if !exists {
		home = "/home"
	}

	configPath := filepath.Join(home, ".kube", "config")
	// use the current context in kubeconfig
	config, err := clientcmd.BuildConfigFromFlags("", configPath)

	if _, err := os.Stat(configPath); os.IsNotExist(err) {
		return nil, KubeContext{}, fmt.Errorf("kubeconfig file not found at %s", configPath)
	}

	var context KubeContext
	if contextConfig := clientcmd.GetConfigFromFileOrDie(configPath); contextConfig.CurrentContext != "" {
		ctxName := contextConfig.CurrentContext

		kubeCtx := contextConfig.Contexts[ctxName]
		clusterName := kubeCtx.Cluster

		clusterInfo, ok := contextConfig.Clusters[clusterName]
		var server string
		var serverCA []uint8
		if !ok {
			slog.Warn("Couldn't get kubeconfig cluster of context " + ctxName)
		} else {
			server = clusterInfo.Server
			serverCA = clusterInfo.CertificateAuthorityData
		}

		authInfo, ok := contextConfig.AuthInfos[kubeCtx.AuthInfo]
		if !ok {
			slog.Warn("Couldn't get auth kubeconfig of context " + ctxName)
		}

		context = KubeContext{
			Name:     ctxName,
			UserCert: authInfo.ClientCertificateData,
			UserKey:  authInfo.ClientCertificateData,
			AuthExec: authInfo.Exec != nil,
			Server:   server,
			ServerCA: serverCA,
		}
	}

	return config, context, err
}

type K8sClient struct {
	*kubernetes.Clientset
	Config  *restclient.Config
	Context KubeContext
}

func (c K8sClient) Valid() bool {
	return c.Clientset != nil
}

func (client K8sClient) GetApiServer() (domain.ApiServer, error) {
	// extract the HOST of the url which is an ip address
	apiServerIP := strings.Split(client.Config.Host, ":")[1][2:]
	apiServerIPAddr, err := net.ResolveIPAddr("ip", apiServerIP)
	if err != nil {
		return domain.ApiServer{}, err
	}

	name := "#API Server"
	ns := "kube-system"
	p := domain.NewPod(name, ns)

	p.K8sEntity.Owner = domain.OwnerRef{
		Uid:  fmt.Sprintf("ns/%s/wl/%s", ns, name),
		Kind: "AbstractWorkload",
		Name: name,
	}
	apiServerPod := domain.ApiServer{
		Pod:        p,
		ExternalIP: *apiServerIPAddr,
		CAData:     client.Config.CAData,
	}
	return apiServerPod, nil
}

func NewK8sClient(kubeConfigPath string) (K8sClient, error) {
	config, context, err := GetConfig()
	if err != nil {
		return K8sClient{}, err
	}

	c := K8sClient{
		Config:  config,
		Context: context,
	}

	clientset, err := kubernetes.NewForConfig(config)
	if err != nil {
		return c, err
	}
	c.Clientset = clientset

	return c, nil
}

func (c K8sClient) TestConnection() error {
	timeout := time.Second * 2
	ctxWithTimeout, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()

	res := c.Discovery().RESTClient().Get().AbsPath("/readyz").Do(ctxWithTimeout)
	if res.Error() != nil {
		return res.Error()
	}

	slog.Debug("Target cluster is readyz")

	return nil
}

func GetIDsOfRunningPods(ctx context.Context, ns string) ([]string, error) {
	// empty NS = all namespaces
	client, err := NewK8sClient("")
	if err != nil {
		return nil, fmt.Errorf("could not create K8s client: %v", err)
	}
	pods, err := client.GetPods(ctx, ns)
	if err != nil {
		return nil, fmt.Errorf("could not get running pods: %v", err)
	}

	podIds := []string{}
	hiddenNamespaces := []string{"kube-system", "local-path-storage"}

	for _, p := range pods {
		if !slices.Contains(hiddenNamespaces, p.GetNamespace()) {
			podIds = append(podIds, p.GetId())
		}
	}
	// TODO: find a good way to sort the Pods
	return podIds, nil
}

func (c K8sClient) GetPod(ctx context.Context, ns, name string) (domain.Pod, error) {
	k8sPod, err := c.CoreV1().Pods(ns).Get(ctx, name, metav1.GetOptions{})
	if err != nil {
		return domain.Pod{}, err
	}

	meta := k8sPod.GetObjectMeta()
	owner := getOwnerReference(meta)

	p := domain.NewPod(meta.GetName(), meta.GetNamespace())
	p.K8sEntity.Id = string(meta.GetUID())
	p.K8sEntity.Labels = meta.GetLabels()
	p.K8sEntity.Annotations = meta.GetAnnotations()
	p.K8sEntity.Owner = owner
	p.Spec = k8sPod.Spec

	return p, nil
}

func (c K8sClient) GetDeployments(ctx context.Context, ns string) ([]domain.Deployment, error) {
	k8sDeployments, err := c.AppsV1().Deployments(ns).List(ctx, metav1.ListOptions{})
	if err != nil {
		return nil, err
	}

	depls := make([]domain.Deployment, 0)
	for _, depl := range k8sDeployments.Items {
		meta := depl.GetObjectMeta()
		depls = append(depls, domain.Deployment{
			K8sEntity: domain.K8sEntity{
				Id:          string(meta.GetUID()),
				Name:        meta.GetName(),
				Namespace:   meta.GetNamespace(),
				Kind:        "Deployment",
				Labels:      meta.GetLabels(),
				Annotations: meta.GetAnnotations(),
			},
			ResourceOwner: domain.ResourceOwner{
				Pods: make([]domain.Pod, 0),
			},
			// Spec:        depl.Spec,
			// Owner:       owner,
		})
	}
	return depls, nil
}

func getOwnerReference(meta metav1.Object) domain.OwnerRef {
	var owner domain.OwnerRef
	if len(meta.GetOwnerReferences()) == 0 {
		return owner
	}

	ownerKind := meta.GetOwnerReferences()[0].Kind
	ownerName := meta.GetOwnerReferences()[0].Name

	if ownerKind == "ReplicaSet" {
		// skip RS and instead reference the deployment with unknown UID
		parts := strings.Split(ownerName, "-")
		// drop the last part of a string where all parts are separated by '-'
		name := strings.Join(parts[:len(parts)-1], "-")
		owner = domain.OwnerRef{
			Name: name,
			Kind: "Deployment",
		}
	} else {
		owner = domain.OwnerRef{
			Name: ownerName,
			Kind: ownerKind,
			Uid:  string(meta.GetOwnerReferences()[0].UID),
		}
	}

	return owner
}

func (c K8sClient) GetPods(ctx context.Context, ns string) ([]domain.Pod, error) {
	k8sPods, err := c.CoreV1().Pods(ns).List(ctx, metav1.ListOptions{})
	if err != nil {
		return nil, err
	}

	pods := make([]domain.Pod, 0)
	for _, pod := range k8sPods.Items {
		if pod.Status.Phase != "Running" {
			continue
		}

		meta := pod.GetObjectMeta()
		owner := getOwnerReference(meta)

		p := domain.NewPod(meta.GetName(), meta.GetNamespace())
		p.K8sEntity.Id = string(meta.GetUID())
		p.K8sEntity.Labels = meta.GetLabels()
		p.K8sEntity.Annotations = meta.GetAnnotations()
		p.K8sEntity.Owner = owner
		p.Spec = pod.Spec

		pods = append(pods, p)
	}
	return pods, nil
}

func ExecInPod(ctx context.Context, client K8sClient, podName, ns, cmd string) (string, string, error) {
	req := client.CoreV1().RESTClient().Post().
		Resource("pods").
		Name(podName).
		Namespace(ns).
		SubResource("exec")

	// scheme := runtime.NewScheme()
	// if err := core_v1.AddToScheme(scheme); err != nil {
	// 	return "", "", fmt.Errorf("error adding to scheme: %v", err)
	// }

	kubeCfg := clientcmd.NewNonInteractiveDeferredLoadingClientConfig(
		clientcmd.NewDefaultClientConfigLoadingRules(),
		&clientcmd.ConfigOverrides{},
	)
	config, err := kubeCfg.ClientConfig()
	if err != nil {
		return "", "", err
	}
	command := []string{"sh", "-c", cmd}

	// parameterCodec := runtime.NewParameterCodec(scheme)
	req.VersionedParams(&v1.PodExecOptions{
		Command: command,
		// Container: containerName,
		Stdin:  false,
		Stdout: true,
		Stderr: true,
		TTY:    false,
	}, scheme.ParameterCodec)
	// }, parameterCodec)

	// if debug {
	// 	fmt.Println("Request URL:", req.URL().String())
	// }

	exec, err := remotecommand.NewSPDYExecutor(config, "POST", req.URL())
	if err != nil {
		return "", "", fmt.Errorf("error while creating Executor: %v", err)
	}

	var stdout, stderr bytes.Buffer
	err = exec.StreamWithContext(ctx, remotecommand.StreamOptions{
		// Stdin:  stdin,
		Stdout: &stdout,
		Stderr: &stderr,
		Tty:    false,
	})
	return strings.TrimSpace(stdout.String()), strings.TrimSpace(stderr.String()), err
}

func DeployPod(ctx context.Context, client K8sClient, podName, ns string, cfg domain.PodConfig) (string, error) {
	// TODO: sysctls are on the PodSecurityContext
	pod := &v1.Pod{
		ObjectMeta: metav1.ObjectMeta{Name: podName},
		Spec: v1.PodSpec{
			RestartPolicy: v1.RestartPolicyNever,
			HostPID:       cfg.HostPID,
			HostNetwork:   cfg.HostNetwork,
			HostIPC:       cfg.HostIPC,
			NodeName:      cfg.NodeName,
			Containers: []v1.Container{{
				Name:    podName,
				Image:   cfg.Image,
				Command: strings.Fields(cfg.Command),
				// Args:    []string{"-c", "print()"},
				SecurityContext: &v1.SecurityContext{
					Privileged: &cfg.Privileged,
				},
			}},
		},
	}

	p, err := client.CoreV1().Pods(ns).Create(
		context.Background(),
		pod,
		metav1.CreateOptions{},
	)
	if err != nil {
		return "", err
	}
	return p.Status.String(), nil
}

func ParseStatus(jsonStr string) (*metav1.Status, error) {
	var status metav1.Status
	err := json.Unmarshal([]byte(jsonStr), &status)
	return &status, err
}

// ParsePodList converts a JSON string containing a PodList into a v1.PodList object.
func ParsePodList(jsonStr string) (*v1.PodList, error) {
	var list v1.PodList
	err := json.Unmarshal([]byte(jsonStr), &list)
	return &list, err
}

// ParseDeploymentList converts a JSON string containing a DeploymentList into an appsV1.DeploymentList object.
func ParseDeploymentList(jsonStr string) (*appsV1.DeploymentList, error) {
	var list appsV1.DeploymentList
	err := json.Unmarshal([]byte(jsonStr), &list)
	return &list, err
}

// ParseServiceAccountList converts a JSON string containing a PodList into a v1.PodList object.
func ParseServiceAccountList(jsonStr string) (*v1.ServiceAccountList, error) {
	var list v1.ServiceAccountList
	err := json.Unmarshal([]byte(jsonStr), &list)
	return &list, err
}

func ParseSecretList(jsonStr string) (*v1.SecretList, error) {
	var list v1.SecretList
	err := json.Unmarshal([]byte(jsonStr), &list)
	return &list, err
}

func ParseConfigMapList(jsonStr string) (*v1.ConfigMapList, error) {
	var list v1.ConfigMapList
	err := json.Unmarshal([]byte(jsonStr), &list)
	return &list, err
}

func ParseNodeList(jsonStr string) (*v1.NodeList, error) {
	var list v1.NodeList
	err := json.Unmarshal([]byte(jsonStr), &list)
	return &list, err
}

// func StreamToPod(k8sClient *kubernetes.Clientset, config *rest.Config, commandChan <-chan string) {
// 	// 1. Create a pipe to act as Stdin
// 	reader, writer := io.Pipe()

// 	// 2. Set up the Exec Request
// 	req := k8sClient.CoreV1().RESTClient().Post().
// 		Resource("pods").
// 		Name("my-pod-name").
// 		Namespace("default").
// 		SubResource("exec")

// 	option := &v1.PodExecOptions{
// 		Command: []string{"/bin/sh"}, // Start a persistent shell
// 		Stdin:   true,
// 		Stdout:  true,
// 		Stderr:  true,
// 		TTY:     true,
// 	}
// 	req.VersionedParams(option, scheme.ParameterCodec)

// 	// 3. Initialize the Executor
// 	exec, _ := remotecommand.NewSPDYExecutor(config, "POST", req.URL())

// 	// 4. Run the stream in a goroutine
// 	go func() {
// 		err := exec.Stream(remotecommand.StreamOptions{
// 			Stdin:  reader,
// 			Stdout: os.Stdout,
// 			Stderr: os.Stderr,
// 			Tty:    true,
// 		})
// 		if err != nil {
// 			fmt.Printf("Stream failed: %v\n", err)
// 		}
// 	}()

// 	// 5. The "Control Loop"
// 	// This listens to your channel and pushes data into the shell's Stdin
// 	for cmd := range commandChan {
// 		fmt.Fprintf(writer, "%s\n", cmd)
// 	}
// }

func PersistentExec(ctx context.Context, clientset *kubernetes.Clientset, config *rest.Config, podName, namespace string, cmdChan <-chan string) error {
	// 1. Prepare the API request
	req := clientset.CoreV1().RESTClient().Post().
		Resource("pods").
		Name(podName).
		Namespace(namespace).
		SubResource("exec")

	option := &v1.PodExecOptions{
		Command: []string{"/bin/sh"}, // The persistent shell
		Stdin:   true,
		Stdout:  true,
		Stderr:  true,
		TTY:     false, // Usually false for programmatic handling (no escape codes)
	}
	req.VersionedParams(option, scheme.ParameterCodec)

	// 2. Create the SPDY executor
	exec, err := remotecommand.NewSPDYExecutor(config, "POST", req.URL())
	if err != nil {
		return err
	}

	// 3. Set up the pipes
	stdinReader, stdinWriter := io.Pipe()

	// Optional: Create a custom writer to capture output programmatically
	stdoutWriter := os.Stdout

	// 4. Bridge the Go channel to the Stdin pipe
	go func() {
		defer stdinWriter.Close()
		for {
			select {
			case <-ctx.Done():
				return
			case cmd, ok := <-cmdChan:
				if !ok {
					return
				}
				// Append newline so the shell executes the command
				fmt.Fprintln(stdinWriter, cmd)
			}
		}
	}()

	// 5. Start the stream (this blocks until the shell exits)
	return exec.StreamWithContext(ctx, remotecommand.StreamOptions{
		Stdin:  stdinReader,
		Stdout: stdoutWriter,
		Stderr: os.Stderr,
		Tty:    false,
	})
}
