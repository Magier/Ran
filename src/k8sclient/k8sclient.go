package k8s

import (
	"bytes"
	"context"
	"crypto/tls"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/Magier/Ran/domain"
	v1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/kubernetes/scheme"
	restclient "k8s.io/client-go/rest"
	"k8s.io/client-go/tools/clientcmd"
	"k8s.io/client-go/tools/remotecommand"
)

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
		home = "/root"
	}

	configPath := filepath.Join(home, ".kube", "config")
	// use the current context in kubeconfig
	config, err := clientcmd.BuildConfigFromFlags("", configPath)

	var context KubeContext
	if contextConfig := clientcmd.GetConfigFromFileOrDie(configPath); contextConfig != nil {
		ctxName := contextConfig.CurrentContext

		clusterInfo, ok := contextConfig.Clusters[ctxName]
		if !ok {
			slog.Warn("Couldn't get kubeconfig cluster of context " + ctxName)
		}

		authInfo, ok := contextConfig.AuthInfos[ctxName]
		if !ok {
			slog.Warn("Couldn't get auth kubeconfig of context " + ctxName)
		}

		context = KubeContext{
			Name:     ctxName,
			UserCert: authInfo.ClientCertificateData,
			UserKey:  authInfo.ClientCertificateData,
			AuthExec: authInfo.Exec != nil,
			Server:   clusterInfo.Server,
			ServerCA: clusterInfo.CertificateAuthorityData,
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

func (c K8sClient) TestConnection() bool {
	var timeout time.Duration = time.Second * 2
	url := c.RESTClient().Get().AbsPath("/livez").URL()
	client := http.Client{
		Timeout: timeout,
		Transport: &http.Transport{
			TLSClientConfig: &tls.Config{InsecureSkipVerify: true},
		},
	}
	res, err := client.Get(url.String())
	if err != nil {
		return false
	}
	res.Body.Close()
	return true
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
	if err != nil {
		return "", stderr.String(), err
	}

	return strings.TrimSpace(stdout.String()), strings.TrimSpace(stderr.String()), nil
}

func DeployPod(ctx context.Context, client K8sClient, podName, ns, image, cmd string, hostIPC, hostPID, hostNetwork bool) (string, error) {
	pod := &v1.Pod{
		ObjectMeta: metav1.ObjectMeta{Name: podName},
		Spec: v1.PodSpec{
			RestartPolicy: v1.RestartPolicyNever,
			Containers: []v1.Container{{
				Name:    podName,
				Image:   image,
				Command: strings.Fields(cmd),
				// Args:    []string{"-c", "print()"},
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
