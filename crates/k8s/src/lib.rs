use std::{env, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{AttachParams, ListParams},
    config::{KubeConfigOptions, Kubeconfig},
    runtime::watcher,
    Api, Client, Config,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Cancellation handle for a running pod watch. The background task is aborted when this is
/// dropped.
pub struct WatchHandle(tokio::task::AbortHandle);

/// Output from a pod exec command, including both streams and the exit code.
/// Only infrastructure failures (can't connect, stream errors) produce an `Err`.
/// A non-zero exit code is returned as `Ok` so callers can surface all output.
#[derive(Debug, Clone)]
pub struct PodExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

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

    /// Start a live watch on pods in the given namespace (or all namespaces if `None`/empty).
    /// Calls `on_change` with the current pod list immediately, then again on every
    /// add/update/delete event. The watch runs until the returned `WatchHandle` is dropped.
    pub fn watch_pods<F>(&self, namespace: Option<String>, on_change: F) -> WatchHandle
    where
        F: Fn(Vec<RunningPod>) + Send + 'static,
    {
        let service = self.clone();
        let jh = tokio::spawn(async move {
            let api: Api<Pod> = match namespace.as_deref().filter(|ns| !ns.is_empty()) {
                Some(ns) => Api::namespaced(service.client.clone(), ns),
                None => Api::all(service.client.clone()),
            };

            // Send initial state before entering the watch loop.
            match service.get_running_pods(namespace.as_deref()).await {
                Ok(pods) => on_change(pods),
                Err(e) => tracing::error!("watch_pods: initial list failed: {e}"),
            }

            let stream = watcher(api, watcher::Config::default());
            tokio::pin!(stream);

            let mut consecutive_errors: u32 = 0;
            while let Some(event) = stream.next().await {
                match event {
                    Ok(_) => {
                        consecutive_errors = 0;
                        // Re-list on any event – mirrors the Go WatchPods behaviour.
                        match service.get_running_pods(namespace.as_deref()).await {
                            Ok(pods) => on_change(pods),
                            Err(e) => tracing::error!("watch_pods: re-list failed: {e}"),
                        }
                    }
                    Err(e) => {
                        consecutive_errors += 1;
                        // Cap backoff at 30 s; first error waits 500 ms.
                        let delay_ms = (500u64 * (1u64 << consecutive_errors.min(6))).min(30_000);
                        tracing::warn!(
                            "watch_pods: watcher error (will retry in {}ms, attempt {}): {e}",
                            delay_ms, consecutive_errors
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        });
        WatchHandle(jh.abort_handle())
    }

    pub async fn exec_pod_command(
        &self,
        namespace: &str,
        pod_name: &str,
        command: &str,
    ) -> Result<PodExecOutput> {
        let command = command.trim();
        if command.is_empty() {
            return Err(anyhow!("pod exec command is empty"));
        }

        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let command_vec = vec!["/bin/sh", "-lc", command];
        let attach_params = AttachParams::default()
            .stdout(true)
            .stderr(true)
            .stdin(false)
            .tty(false);

        let mut attached = api
            .exec(pod_name, command_vec, &attach_params)
            .await
            .with_context(|| {
                format!("failed to exec command in pod '{}/{}'", namespace, pod_name)
            })?;

        let mut stdout = String::new();
        if let Some(mut reader) = attached.stdout() {
            reader
                .read_to_string(&mut stdout)
                .await
                .context("failed reading pod exec stdout")?;
        }

        let mut stderr = String::new();
        if let Some(mut reader) = attached.stderr() {
            reader
                .read_to_string(&mut stderr)
                .await
                .context("failed reading pod exec stderr")?;
        }

        let status = attached
            .take_status()
            .ok_or_else(|| anyhow!("missing pod exec status stream"))?
            .await
            .context("failed to receive pod exec status")?;

        let exit_code = if status.status == Some("Success".to_string()) {
            0
        } else {
            parse_exit_code(status.message.as_deref()).unwrap_or(1)
        };

        Ok(PodExecOutput {
            stdout,
            stderr,
            exit_code,
        })
    }

    /// Open a long-lived interactive exec session into a pod and return a
    /// [`tokio::io::DuplexStream`] that acts as the session's stdin/stdout.
    ///
    /// Internally starts `/bin/sh` in the pod with `stdin=true, stdout=true,
    /// tty=false` and bridges the `AttachedProcess` to the duplex stream via a
    /// background proxy task.  Dropping the returned stream tears down the
    /// proxy task and closes the exec channel.
    pub async fn open_exec_session(
        &self,
        namespace: &str,
        pod: &str,
        container: Option<&str>,
    ) -> Result<tokio::io::DuplexStream> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);

        let mut params = AttachParams::default()
            .stdin(true)
            .stdout(true)
            .stderr(false)
            .tty(false);
        if let Some(c) = container {
            params = params.container(c);
        }

        let mut attached = api
            .exec(pod, ["/bin/sh"], &params)
            .await
            .with_context(|| {
                format!("kubectl exec session failed for pod '{}/{}'", namespace, pod)
            })?;

        let mut stdin_w = attached
            .stdin()
            .ok_or_else(|| anyhow!("kubectl exec: stdin channel unavailable"))?;
        let stdout_r = attached
            .stdout()
            .ok_or_else(|| anyhow!("kubectl exec: stdout channel unavailable"))?;

        let (client, server) = tokio::io::duplex(64 * 1024);

        tokio::spawn(async move {
            let (mut server_rx, mut server_tx) = tokio::io::split(server);
            let mut stdout_r = stdout_r;

            let copy_in = async {
                let _ = tokio::io::copy(&mut server_rx, &mut stdin_w).await;
                // Signal EOF to the shell when the client side closes.
                let _ = stdin_w.shutdown().await;
            };
            let copy_out = tokio::io::copy(&mut stdout_r, &mut server_tx);

            tokio::select! {
                _ = copy_in => {}
                _ = copy_out => {}
            }

            drop(attached);
        });

        Ok(client)
    }
}

fn parse_exit_code(message: Option<&str>) -> Option<i32> {
    // Kubernetes status message for non-zero exits: "command terminated with exit code N"
    let msg = message?;
    let code_str = msg.strip_prefix("command terminated with exit code ")?;
    code_str.trim().parse().ok()
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
        .ok_or_else(|| anyhow!("context '{}' does not reference a cluster", context_name))?;

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
