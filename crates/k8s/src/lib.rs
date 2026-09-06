use std::{collections::HashMap, env, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use http::{Method, Request};
use k8s_openapi::api::authorization::v1::{SelfSubjectRulesReview, SelfSubjectRulesReviewSpec};
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{AttachParams, ListParams, PostParams},
    config::{KubeConfigOptions, Kubeconfig},
    Api, Client as KubeClient, Config,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Output from a pod exec command, including both streams and the exit code.
/// Only infrastructure failures (can't connect, stream errors) produce an `Err`.
/// A non-zero exit code is returned as `Ok` so callers can surface all output.
#[derive(Debug, Clone)]
pub struct PodExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
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

/// Context-aware kubeconfig data shared by runtime client construction and
/// campaign knowledge bootstrap.
#[derive(Debug, Clone)]
pub struct ResolvedKubeconfig {
    kubeconfig: Kubeconfig,
    source_path: Option<PathBuf>,
    pub context_name: String,
    /// Explicit default namespace configured on the selected context.
    /// Kubernetes' implicit `default` fallback is intentionally not inferred.
    pub default_namespace: Option<String>,
    pub cluster_name: String,
    pub user_name: Option<String>,
    pub server: Option<String>,
    pub ca_data: Option<String>,
    pub token: Option<String>,
    pub cert_data: Option<String>,
    pub key_data: Option<String>,
    pub auth_method: String,
    pub has_token: bool,
    pub has_client_certificate: bool,
    pub has_client_key: bool,
}

impl ResolvedKubeconfig {
    pub fn target_cluster(&self) -> TargetCluster {
        TargetCluster {
            name: self.cluster_name.clone(),
            context_name: Some(self.context_name.clone()),
            server: self.server.clone(),
        }
    }

    fn options(&self) -> KubeConfigOptions {
        KubeConfigOptions {
            context: Some(self.context_name.clone()),
            cluster: None,
            user: None,
        }
    }
}

/// Resolve one context from a kubeconfig file without constructing a client.
pub fn resolve_kubeconfig(
    path: impl Into<PathBuf>,
    context_override: Option<&str>,
) -> Result<ResolvedKubeconfig> {
    let path = path.into();
    let kubeconfig = Kubeconfig::read_from(path.clone())
        .with_context(|| format!("failed to read kubeconfig at {}", path.display()))?;
    let mut resolved = resolve_kubeconfig_data(kubeconfig, context_override)
        .with_context(|| format!("failed to resolve kubeconfig at {}", path.display()))?;
    resolved.source_path = Some(path);
    Ok(resolved)
}

/// Resolve one context from already-loaded kubeconfig data.
pub fn resolve_kubeconfig_data(
    kubeconfig: Kubeconfig,
    context_override: Option<&str>,
) -> Result<ResolvedKubeconfig> {
    let context_name = context_override
        .map(str::to_string)
        .or_else(|| kubeconfig.current_context.clone())
        .ok_or_else(|| anyhow!("kubeconfig does not define current-context"))?;

    let named_context = kubeconfig
        .contexts
        .iter()
        .find(|ctx| ctx.name == context_name)
        .ok_or_else(|| anyhow!("context '{}' not found in kubeconfig", context_name))?;
    let context = named_context
        .context
        .as_ref()
        .ok_or_else(|| anyhow!("context '{}' has no configuration", context_name))?;

    let cluster_name = context.cluster.clone();
    let cluster = kubeconfig
        .clusters
        .iter()
        .find(|cluster| cluster.name == cluster_name)
        .and_then(|cluster| cluster.cluster.as_ref())
        .ok_or_else(|| {
            anyhow!(
                "cluster '{}' referenced by context '{}' not found in kubeconfig",
                cluster_name,
                context_name
            )
        })?;

    let user_name = context.user.clone();
    let default_namespace = context
        .namespace
        .as_deref()
        .map(str::trim)
        .filter(|namespace| !namespace.is_empty())
        .map(str::to_string);
    let auth = user_name
        .as_deref()
        .and_then(|name| kubeconfig.auth_infos.iter().find(|user| user.name == name))
        .and_then(|user| user.auth_info.as_ref());
    if user_name.is_some() && auth.is_none() {
        return Err(anyhow!(
            "user '{}' referenced by context '{}' not found in kubeconfig",
            user_name.as_deref().unwrap_or_default(),
            context_name
        ));
    }

    let token = auth
        .and_then(|a| a.token.as_ref())
        .map(|value| value.expose_secret().to_string());
    let cert_data = auth.and_then(|a| a.client_certificate_data.clone());
    let key_data = auth
        .and_then(|a| a.client_key_data.as_ref())
        .map(|value| value.expose_secret().to_string());
    let has_token = token.is_some() || auth.is_some_and(|a| a.token_file.is_some());
    let has_client_certificate =
        cert_data.is_some() || auth.is_some_and(|a| a.client_certificate.is_some());
    let has_client_key = key_data.is_some() || auth.is_some_and(|a| a.client_key.is_some());
    let auth_method = match auth {
        Some(a) if a.exec.is_some() => "exec",
        Some(a) if a.auth_provider.is_some() => "auth-provider",
        Some(_) if has_token => "token",
        Some(_) if has_client_certificate || has_client_key => "client-certificate",
        Some(a) if a.username.is_some() || a.password.is_some() => "basic",
        Some(_) => "unknown",
        None => "anonymous",
    }
    .to_string();
    let server = cluster.server.clone();
    let ca_data = cluster.certificate_authority_data.clone();

    Ok(ResolvedKubeconfig {
        kubeconfig,
        source_path: None,
        context_name,
        default_namespace,
        cluster_name,
        user_name,
        server,
        ca_data,
        token,
        cert_data,
        key_data,
        auth_method,
        has_token,
        has_client_certificate,
        has_client_key,
    })
}

/// Parse kubeconfig YAML and resolve its selected context using the same rules
/// as file-backed runtime configuration.
pub fn resolve_kubeconfig_yaml(
    content: &str,
    context_override: Option<&str>,
) -> Result<ResolvedKubeconfig> {
    let kubeconfig: Kubeconfig =
        serde_yaml::from_str(content).context("failed to parse kubeconfig YAML")?;
    resolve_kubeconfig_data(kubeconfig, context_override)
}

fn pod_to_running_pod(pod: &Pod) -> Option<RunningPod> {
    let name = pod.metadata.name.clone().unwrap_or_default();
    let namespace = pod.metadata.namespace.clone().unwrap_or_default();
    let phase = pod.status.as_ref().and_then(|s| s.phase.clone())?;
    if phase != "Running" {
        return None;
    }

    let not_ready_cs = pod
        .status
        .as_ref()
        .and_then(|s| s.container_statuses.as_deref())
        .unwrap_or_default()
        .iter()
        .find(|cs| !cs.ready);

    let (ready, state_reason) = match not_ready_cs {
        None => (true, None),
        Some(cs) => {
            let reason = cs.state.as_ref().and_then(|s| {
                s.waiting
                    .as_ref()
                    .and_then(|w| w.reason.clone())
                    .or_else(|| s.terminated.as_ref().and_then(|t| t.reason.clone()))
            });
            (false, reason)
        }
    };

    Some(RunningPod {
        id: format!("ns/{}/pod/{}", namespace, name),
        name,
        namespace: if namespace.is_empty() {
            None
        } else {
            Some(namespace)
        },
        phase: Some(phase),
        ready: Some(ready),
        state_reason,
    })
}

#[derive(Clone)]
pub struct Client {
    client: KubeClient,
    kubeconfig_path: Option<PathBuf>,
    api_server: String,
}

fn build_authenticated_http_request(request: &serde_json::Value) -> Result<Request<Vec<u8>>> {
    #[derive(Deserialize)]
    struct Spec {
        authentication: String,
        url: String,
        #[serde(default = "default_method")]
        method: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        #[serde(default)]
        body: String,
    }
    fn default_method() -> String {
        "GET".to_string()
    }

    let spec: Spec = serde_json::from_value(request.clone())
        .context("invalid authenticated Kubernetes HTTP request")?;
    if spec.authentication.trim().is_empty() {
        return Err(anyhow!(
            "Kubernetes HTTP request is missing explicit authentication"
        ));
    }
    if spec
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("authorization"))
    {
        return Err(anyhow!(
            "Kubernetes HTTP request must not supply its own Authorization header"
        ));
    }

    let authored_uri: http::Uri = spec
        .url
        .trim()
        .parse()
        .with_context(|| format!("invalid Kubernetes HTTP URL '{}'", spec.url.trim()))?;
    let path = authored_uri
        .path_and_query()
        .map(|value| value.as_str())
        .filter(|value| value.starts_with('/'))
        .ok_or_else(|| anyhow!("Kubernetes HTTP URL must include an absolute API path"))?;
    let method = Method::from_bytes(spec.method.trim().as_bytes())
        .with_context(|| format!("invalid Kubernetes HTTP method '{}'", spec.method))?;

    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in spec.headers {
        builder = builder.header(name, value);
    }
    builder
        .body(spec.body.into_bytes())
        .context("failed to build Kubernetes HTTP request")
}

impl Client {
    pub async fn from_kubeconfig(kubeconfig: Option<PathBuf>) -> Result<Self> {
        let path = kubeconfig.unwrap_or_else(default_kubeconfig_path);
        let resolved = resolve_kubeconfig(path, None)?;
        Self::from_resolved_kubeconfig(&resolved).await
    }

    pub async fn from_resolved_kubeconfig(resolved: &ResolvedKubeconfig) -> Result<Self> {
        let config =
            Config::from_custom_kubeconfig(resolved.kubeconfig.clone(), &resolved.options())
                .await
                .context("failed to load Kubernetes config from kubeconfig")?;
        let client = KubeClient::try_from(config).context("failed to create Kubernetes client")?;

        Ok(Self {
            client,
            kubeconfig_path: resolved.source_path.clone(),
            api_server: resolved
                .server
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        })
    }

    /// Path to the kubeconfig file that backs this client, when it is
    /// file-backed. Used by local-host TTPs (e.g. Read Local Kubeconfig) to
    /// read the same kubeconfig Ran was configured with.
    pub fn kubeconfig_path(&self) -> Option<&std::path::Path> {
        self.kubeconfig_path.as_deref()
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

        Ok(pods.iter().filter_map(pod_to_running_pod).collect())
    }

    /// Ask Kubernetes which RBAC rules apply to the identity represented by
    /// this client's kubeconfig. The native response shape is retained so the
    /// campaign can reuse its existing SelfSubjectRulesReview parser.
    pub async fn self_subject_rules_review(&self, namespace: &str) -> Result<String> {
        let reviews: Api<SelfSubjectRulesReview> = Api::all(self.client.clone());
        let review = SelfSubjectRulesReview {
            spec: SelfSubjectRulesReviewSpec {
                namespace: Some(namespace.to_string()),
            },
            ..Default::default()
        };
        let response = reviews
            .create(&PostParams::default(), &review)
            .await
            .with_context(|| {
                format!(
                    "could not complete SelfSubjectRulesReview request to Kubernetes API server '{}' for namespace '{}'",
                    self.api_server, namespace
                )
            })?;
        serde_json::to_string(&response)
            .context("failed to serialize SelfSubjectRulesReview response")
    }

    /// Execute a grounded structured Kubernetes request with this client's
    /// authentication and TLS configuration. Authentication fields embedded in
    /// the request description are deliberately ignored.
    pub async fn execute_request(&self, request: &serde_json::Value) -> Result<String> {
        #[derive(Deserialize)]
        struct Spec {
            api: String,
            resource: String,
            #[serde(default)]
            namespace: String,
            #[serde(default)]
            cluster_scoped: serde_json::Value,
            #[serde(default)]
            query: String,
            #[serde(default = "default_method")]
            method: String,
            #[serde(default)]
            body: serde_json::Value,
        }
        fn default_method() -> String {
            "GET".to_string()
        }
        fn is_true(value: &serde_json::Value) -> bool {
            value.as_bool().unwrap_or_else(|| {
                value
                    .as_str()
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"))
            })
        }

        let spec: Spec = serde_json::from_value(request.clone())
            .context("invalid structured Kubernetes request")?;
        let api = spec.api.trim_matches('/');
        let resource = spec.resource.trim_matches('/');
        let mut path = if is_true(&spec.cluster_scoped) || spec.namespace.trim().is_empty() {
            format!("/{api}/{resource}")
        } else {
            format!("/{api}/namespaces/{}/{resource}", spec.namespace.trim())
        };
        if !spec.query.trim().is_empty() {
            path.push('?');
            path.push_str(spec.query.trim());
        }
        let method = Method::from_bytes(spec.method.trim().as_bytes())
            .with_context(|| format!("invalid Kubernetes request method '{}'", spec.method))?;
        let body = if spec.body.is_null() {
            Vec::new()
        } else {
            serde_json::to_vec(&spec.body).context("failed to encode Kubernetes request body")?
        };
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(body)
            .context("failed to build Kubernetes request")?;
        self.client
            .request_text(request)
            .await
            .context("Kubernetes API request failed")
    }

    /// Execute a general HTTP-form Kubernetes procedure through this client's
    /// configured API server, authentication, and TLS stack. The authored URL
    /// contributes only its path and query; the active kubeconfig selects the
    /// server and supplies authentication.
    pub async fn execute_authenticated_http_request(
        &self,
        request: &serde_json::Value,
    ) -> Result<String> {
        let request = build_authenticated_http_request(request)?;
        self.client
            .request_text(request)
            .await
            .context("Kubernetes HTTP request failed")
    }

    /// Run a kubectl procedure on the Ran host. `${K8S_AUTH}` grounding emits
    /// an explicit `--kubeconfig "$KUBECONFIG"` flag while the actual path is
    /// supplied only through this process environment.
    pub async fn execute_kubectl_command(&self, command: &str) -> Result<PodExecOutput> {
        let kubeconfig = self
            .kubeconfig_path
            .as_ref()
            .ok_or_else(|| anyhow!("active Kubernetes client has no file-backed kubeconfig"))?;
        let output = tokio::process::Command::new("/bin/sh")
            .arg("-lc")
            .arg(command)
            .env("KUBECONFIG", kubeconfig)
            .output()
            .await
            .context("failed to execute kubectl procedure")?;
        Ok(PodExecOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(1),
        })
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

        let mut attached = api.exec(pod, ["/bin/sh"], &params).await.with_context(|| {
            format!(
                "kubectl exec session failed for pod '{}/{}'",
                namespace, pod
            )
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
    Ok(resolve_kubeconfig(path, None)?.target_cluster())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KUBECONFIG: &str = r#"apiVersion: v1
kind: Config
clusters:
- name: cluster-a
  cluster:
    server: https://a.example
- name: cluster-b
  cluster:
    server: https://b.example
contexts:
- name: context-a
  context:
    cluster: cluster-a
    user: user-a
    namespace: default
- name: context-b
  context:
    cluster: cluster-b
    user: user-b
    namespace: "   "
current-context: context-a
users:
- name: user-a
  user:
    token: secret-a
- name: user-b
  user:
    client-certificate-data: CERT
    client-key-data: KEY
"#;

    #[test]
    fn resolver_uses_current_context_by_default() {
        let resolved = resolve_kubeconfig_yaml(KUBECONFIG, None).unwrap();
        assert_eq!(resolved.context_name, "context-a");
        assert_eq!(resolved.default_namespace.as_deref(), Some("default"));
        assert_eq!(resolved.cluster_name, "cluster-a");
        assert_eq!(resolved.user_name.as_deref(), Some("user-a"));
        assert_eq!(resolved.server.as_deref(), Some("https://a.example"));
        assert!(resolved.has_token);
        assert_eq!(resolved.auth_method, "token");
    }

    #[test]
    fn resolver_honors_context_override() {
        let resolved = resolve_kubeconfig_yaml(KUBECONFIG, Some("context-b")).unwrap();
        assert_eq!(resolved.cluster_name, "cluster-b");
        assert_eq!(resolved.default_namespace, None);
        assert_eq!(resolved.user_name.as_deref(), Some("user-b"));
        assert!(resolved.has_client_certificate);
        assert!(resolved.has_client_key);
        assert_eq!(resolved.auth_method, "client-certificate");
    }

    #[test]
    fn resolver_rejects_missing_context_and_user() {
        assert!(resolve_kubeconfig_yaml(KUBECONFIG, Some("missing")).is_err());
        let missing_user = KUBECONFIG.replace("user: user-a", "user: missing");
        assert!(resolve_kubeconfig_yaml(&missing_user, None).is_err());
    }

    #[test]
    fn authenticated_http_request_preserves_method_path_headers_and_body() {
        let request = build_authenticated_http_request(&serde_json::json!({
            "authentication": "--kubeconfig \"$KUBECONFIG\"",
            "method": "POST",
            "url": "https://ignored.example/apis/authorization.k8s.io/v1/selfsubjectrulesreviews?watch=false",
            "headers": {"Content-Type": "application/json"},
            "body": "{\"kind\":\"SelfSubjectRulesReview\"}"
        }))
        .unwrap();

        assert_eq!(request.method(), Method::POST);
        assert_eq!(
            request.uri().path_and_query().unwrap().as_str(),
            "/apis/authorization.k8s.io/v1/selfsubjectrulesreviews?watch=false"
        );
        assert_eq!(request.headers()["Content-Type"], "application/json");
        assert_eq!(request.body(), br#"{"kind":"SelfSubjectRulesReview"}"#);
    }

    #[test]
    fn authenticated_http_request_rejects_authored_authorization_header() {
        let result = build_authenticated_http_request(&serde_json::json!({
            "authentication": "${K8S_AUTH}",
            "url": "https://cluster.example/api/v1/pods",
            "headers": {"authorization": "Bearer hidden"}
        }));

        assert!(result.is_err());
    }
}
