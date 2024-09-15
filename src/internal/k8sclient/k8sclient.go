package k8s

import (
	"context"
	"os"
	"path/filepath"
	"strings"

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

func GetDeployments(ctx context.Context, clientset *kubernetes.Clientset) ([]domain.Deployment, error) {
	k8sDeployments, err := clientset.AppsV1().Deployments("").List(ctx, metav1.ListOptions{})
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
				Kind:        "Deployment",
				Labels:      meta.GetLabels(),
				Annotations: meta.GetAnnotations(),
			},
			NamespacedResource: domain.NamespacedResource{
				Namespace: meta.GetNamespace(),
			},
			// Spec:        depl.Spec,
			// Owner:       owner,
		})
	}
	return depls, nil
}

func GetPods(ctx context.Context, clientset *kubernetes.Clientset) ([]domain.Pod, error) {
	k8sPods, err := clientset.CoreV1().Pods("").List(ctx, metav1.ListOptions{})
	if err != nil {
		return nil, err
	}

	pods := make([]domain.Pod, 0)
	for _, pod := range k8sPods.Items {
		meta := pod.GetObjectMeta()
		ownerKind := meta.GetOwnerReferences()[0].Kind
		ownerName := meta.GetOwnerReferences()[0].Name
		var owner domain.OwnerRef

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

		pods = append(pods, domain.Pod{
			K8sEntity: domain.K8sEntity{
				Id:          string(meta.GetUID()),
				Name:        meta.GetName(),
				Kind:        "Pod",
				Labels:      meta.GetLabels(),
				Annotations: meta.GetAnnotations(),
				Owner:       owner,
			},
			NamespacedResource: domain.NamespacedResource{
				Namespace: meta.GetNamespace(),
			},
			Spec: pod.Spec,
		})
	}
	return pods, nil
}
