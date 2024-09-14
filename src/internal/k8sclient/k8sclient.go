package k8s

import (
	"context"
	"os"
	"path/filepath"

	"github.com/Magier/Ran/domain"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/client-go/kubernetes"
	restclient "k8s.io/client-go/rest"
	"k8s.io/client-go/tools/clientcmd"
)

func GetConfig() (*restclient.Config, error) {
	home, exists := os.LookupEnv("HOME")
	if !exists {
		home = "/root"
	}

	configPath := filepath.Join(home, ".kube", "config")
	// use the current context in kubeconfig
	config, err := clientcmd.BuildConfigFromFlags("", configPath)
	return config, err
}

func NewK8sClient(kubeConfigPath string) (*kubernetes.Clientset, error) {
	config, err := GetConfig()
	if err != nil {
		return nil, err
	}
	clientset, err := kubernetes.NewForConfig(config)
	if err != nil {
		return nil, err
	}
	return clientset, nil
}

func GetPods(ctx context.Context, clientset *kubernetes.Clientset) ([]domain.Pod, error) {
	k8sPods, err := clientset.CoreV1().Pods("").List(ctx, metav1.ListOptions{})
	if err != nil {
		return nil, err
	}

	pods := make([]domain.Pod, 0)
	for _, pod := range k8sPods.Items {
		meta := pod.GetObjectMeta()
		pods = append(pods, domain.Pod{
			Id:          string(meta.GetUID()),
			Name:        meta.GetName(),
			Namespace:   meta.GetNamespace(),
			Labels:      meta.GetLabels(),
			Annotations: meta.GetAnnotations(),
			Spec:        pod.Spec,
		})
	}
	return pods, nil
}
