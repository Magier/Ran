use std::{env, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::ListParams,
    config::{KubeConfigOptions, Kubeconfig},
    Api, Client, Config,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningPod {
    pub id: String,
    pub name: String,
    pub namespace: Option<String>,
    pub phase: Option<String>,
    pub ready: Option<bool>,
    pub state_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetCluster {
    pub name: String,
    pub context_name: Option<String>,
    pub server: Option<String>,
}

#[derive(Clone)]
pub struct K8sService {
    client: Client,
}

impl K8sService {
    pub async fn from_kubeconfig(kubeconfig: Option<PathBuf>) -> Result<Self> {
        let path = kubeconfig.unwrap_or_else(default_kubeconfig_path);
        let kubeconfig = Kubeconfig::read_from(path.clone())
            .with_context(|| format!("failed to read kubeconfig at {}", path.display()))?;

        let opts = KubeConfigOptions::default();
        let config = Config::from_custom_kubeconfig(kubeconfig, &opts)
            .await
            .context("failed to load Kubernetes config from kubeconfig")?;
        let client = Client::try_from(config).context("failed to create Kubernetes client")?;

        Ok(Self { client })
    }

    pub async fn get_running_pods(&self, namespace: Option<&str>) -> Result<Vec<RunningPod>> {
        let pods = if let Some(ns) = namespace.filter(|v| !v.is_empty()) {
            let api: Api<Pod> = Api::namespaced(self.client.clone(), ns);
            api.list(&ListParams::default())
                .await
                .with_context(|| format!("failed to list pods in namespace '{}'", ns))?
                .items
        } else {
            let api: Api<Pod> = Api::all(self.client.clone());
            api.list(&ListParams::default())
                .await
                .context("failed to list pods in all namespaces")?
                .items
        };

        let mut out = Vec::new();
        for pod in pods {
            let name = pod.metadata.name.unwrap_or_default();
            let namespace = pod.metadata.namespace.clone().unwrap_or_default();
            let phase = pod.status.as_ref().and_then(|s| s.phase.clone());

            if phase.as_deref() != Some("Running") {
                continue;
            }

            let mut ready = true;
            let mut state_reason: Option<String> = None;

            if let Some(status) = &pod.status {
                if let Some(container_statuses) = &status.container_statuses {
                    for cs in container_statuses {
                        if !cs.ready {
                            ready = false;
                            if let Some(state) = &cs.state {
                                if let Some(waiting) = &state.waiting {
                                    if let Some(reason) = &waiting.reason {
                                        state_reason = Some(reason.clone());
                                        break;
                                    }
                                }
                                if let Some(terminated) = &state.terminated {
                                    if let Some(reason) = &terminated.reason {
                                        state_reason = Some(reason.clone());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            out.push(RunningPod {
                id: format!("ns/{}/pod/{}", namespace, name),
                name,
                namespace: if namespace.is_empty() {
                    None
                } else {
                    Some(namespace)
                },
                phase,
                ready: Some(ready),
                state_reason,
            });
        }

        Ok(out)
    }
}

pub fn default_kubeconfig_path() -> PathBuf {
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".kube/config");
    }
    PathBuf::from(".kube/config")
}

pub fn kubeconfig_path_or_err(path: Option<PathBuf>) -> Result<PathBuf> {
    let p = path.unwrap_or_else(default_kubeconfig_path);
    if !p.exists() {
        return Err(anyhow!("kubeconfig file not found at {}", p.display()));
    }
    Ok(p)
}

pub fn target_cluster_from_kubeconfig(path: Option<PathBuf>) -> Result<TargetCluster> {
    let path = kubeconfig_path_or_err(path)?;
    let kubeconfig = Kubeconfig::read_from(path.clone())
        .with_context(|| format!("failed to read kubeconfig at {}", path.display()))?;

    let context_name = kubeconfig
        .current_context
        .clone()
        .ok_or_else(|| anyhow!("kubeconfig does not define current-context"))?;

    let context = kubeconfig
        .contexts
        .iter()
        .find(|ctx| ctx.name == context_name)
        .ok_or_else(|| anyhow!("current-context '{}' not found in kubeconfig", context_name))?;

    let cluster_name = context
        .context
        .as_ref()
        .map(|ctx| ctx.cluster.clone())
        .ok_or_else(|| {
            anyhow!(
                "context '{}' does not reference a cluster",
                context_name
            )
        })?;

    let server = kubeconfig
        .clusters
        .iter()
        .find(|cluster| cluster.name == cluster_name)
        .and_then(|cluster| cluster.cluster.as_ref())
        .and_then(|cluster| cluster.server.clone());

    Ok(TargetCluster {
        name: cluster_name,
        context_name: Some(context_name),
        server,
    })
}
