use std::sync::Arc;

use async_trait::async_trait;
use k8s::{K8sService, PodExecOutput};
use tracing::{debug, warn};

use crate::types::{ExecTtp, TtpExecuted};

#[async_trait]
pub(crate) trait PodExecClient: Send + Sync {
    async fn exec_pod_command(
        &self,
        namespace: &str,
        pod_name: &str,
        command: &str,
    ) -> anyhow::Result<PodExecOutput>;
}

#[async_trait]
impl PodExecClient for K8sService {
    async fn exec_pod_command(
        &self,
        namespace: &str,
        pod_name: &str,
        command: &str,
    ) -> anyhow::Result<PodExecOutput> {
        self.exec_pod_command(namespace, pod_name, command).await
    }
}

#[derive(Clone)]
pub struct BuiltinC2 {
    pod_exec_client: Arc<dyn PodExecClient>,
}

impl BuiltinC2 {
    pub fn new(k8s: K8sService) -> Self {
        Self {
            pod_exec_client: Arc::new(k8s),
        }
    }

    #[cfg(test)]
    fn from_pod_exec_client(client: Arc<dyn PodExecClient>) -> Self {
        Self {
            pod_exec_client: client,
        }
    }

    pub async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
        let command = cmd.procedure.command.trim();

        if command.starts_with("setTarget(") {
            debug!(cmd_id = %cmd.id, "builtin c2 accepted setTarget procedure");
            return ok_result(&cmd.id, "ok".to_string());
        }

        if let Some((namespace, pod_name)) = parse_pod_target_id(&cmd.target_id) {
            debug!(
                cmd_id = %cmd.id,
                target_id = %cmd.target_id,
                namespace,
                pod_name,
                "builtin c2 executing command through pod exec"
            );

            match self
                .pod_exec_client
                .exec_pod_command(namespace, pod_name, &cmd.procedure.command)
                .await
            {
                Ok(output) if output.exit_code == 0 => {
                    let combined = match (
                        output.stdout.trim().is_empty(),
                        output.stderr.trim().is_empty(),
                    ) {
                        (false, false) => format!(
                            "{}\n{}",
                            output.stdout.trim_end(),
                            output.stderr.trim_end()
                        ),
                        (true, false) => output.stderr,
                        (_, _) => output.stdout,
                    };
                    return ok_result(&cmd.id, combined);
                }
                Ok(output) => {
                    let mut results = Vec::new();
                    if !output.stdout.trim().is_empty() {
                        results.push(output.stdout.trim().to_string());
                    }
                    if !output.stderr.trim().is_empty() {
                        results.push(output.stderr.trim().to_string());
                    }
                    let fail_reason = if !output.stderr.trim().is_empty() {
                        output.stderr.trim().to_string()
                    } else if !output.stdout.trim().is_empty() {
                        output.stdout.trim().to_string()
                    } else {
                        format!("command exited with code {}", output.exit_code)
                    };
                    warn!(
                        cmd_id = %cmd.id,
                        target_id = %cmd.target_id,
                        exit_code = output.exit_code,
                        stderr = %output.stderr.trim(),
                        "builtin c2 pod exec command failed"
                    );
                    return TtpExecuted {
                        id: cmd.id.clone(),
                        success: false,
                        results,
                        exit_code: output.exit_code,
                        fail_reason,
                    };
                }
                Err(err) => {
                    let reason = err.to_string();
                    warn!(
                        cmd_id = %cmd.id,
                        target_id = %cmd.target_id,
                        error = %reason,
                        "builtin c2 pod exec infrastructure failure"
                    );
                    return TtpExecuted {
                        id: cmd.id.clone(),
                        success: false,
                        results: vec![reason.clone()],
                        exit_code: 1,
                        fail_reason: reason,
                    };
                }
            }
        }

        if cmd.target_id.starts_with("ns/") {
            let reason = format!(
                "invalid pod target id '{}': expected format ns/<namespace>/pod/<pod-name>",
                cmd.target_id
            );
            warn!(cmd_id = %cmd.id, target_id = %cmd.target_id, "{}", reason);
            return TtpExecuted {
                id: cmd.id.clone(),
                success: false,
                results: vec![reason.clone()],
                exit_code: 1,
                fail_reason: reason,
            };
        }

        debug!(
            cmd_id = %cmd.id,
            target_id = %cmd.target_id,
            "builtin c2 received non-pod target; returning compatibility success"
        );
        ok_result(&cmd.id, "ok".to_string())
    }
}

fn parse_pod_target_id(target_id: &str) -> Option<(&str, &str)> {
    let mut parts = target_id.split('/');
    let kind_a = parts.next()?;
    let namespace = parts.next()?;
    let kind_b = parts.next()?;
    let pod_name = parts.next()?;

    if parts.next().is_some() {
        return None;
    }

    if kind_a != "ns" || kind_b != "pod" || namespace.is_empty() || pod_name.is_empty() {
        return None;
    }

    Some((namespace, pod_name))
}

fn ok_result(cmd_id: &str, output: String) -> TtpExecuted {
    let trimmed = output.trim().to_string();
    let payload = if trimmed.is_empty() {
        "ok".to_string()
    } else {
        trimmed
    };

    TtpExecuted {
        id: cmd_id.to_string(),
        success: true,
        results: vec![payload],
        exit_code: 0,
        fail_reason: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use armory::{Procedure, Ttp};

    use k8s::PodExecOutput;

    use super::{BuiltinC2, PodExecClient};
    use crate::types::ExecTtp;

    #[derive(Default)]
    struct FakePodExecClient {
        calls: Mutex<Vec<(String, String, String)>>,
    }

    #[async_trait::async_trait]
    impl PodExecClient for FakePodExecClient {
        async fn exec_pod_command(
            &self,
            namespace: &str,
            pod_name: &str,
            command: &str,
        ) -> anyhow::Result<PodExecOutput> {
            self.calls
                .lock()
                .expect("lock should not be poisoned")
                .push((
                    namespace.to_string(),
                    pod_name.to_string(),
                    command.to_string(),
                ));
            Ok(PodExecOutput {
                stdout: "uid=0(root) gid=0(root)".to_string(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
    }

    #[tokio::test]
    async fn pod_target_executes_command_successfully() {
        let fake = Arc::new(FakePodExecClient::default());
        let builtin = BuiltinC2::from_pod_exec_client(fake.clone());

        let cmd = exec_cmd("ns/default/pod/nginx", "id", "");
        let result = builtin.execute(&cmd).await;

        assert!(result.success);
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.results, vec!["uid=0(root) gid=0(root)"]);

        let calls = fake.calls.lock().expect("lock should not be poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "default");
        assert_eq!(calls[0].1, "nginx");
        assert_eq!(calls[0].2, "id");
    }

    #[tokio::test]
    async fn malformed_pod_target_returns_failure() {
        let fake = Arc::new(FakePodExecClient::default());
        let builtin = BuiltinC2::from_pod_exec_client(fake.clone());

        let cmd = exec_cmd("ns/default/pod", "id", "");
        let result = builtin.execute(&cmd).await;

        assert!(!result.success);
        assert_eq!(result.exit_code, 1);
        assert!(result.fail_reason.contains("invalid pod target id"));

        let calls = fake.calls.lock().expect("lock should not be poisoned");
        assert!(calls.is_empty());
    }

    fn exec_cmd(target_id: &str, command: &str, exec_system_id: &str) -> ExecTtp {
        ExecTtp {
            id: "cmd-1".to_string(),
            started_at_ms: 0,
            ttp: Ttp {
                id: "T0001".to_string(),
                name: "Test TTP".to_string(),
                description: "test".to_string(),
                tactic: "Execution".to_string(),
                techniques: vec![],
                status: "stable".to_string(),
                params: vec![],
                requires: Default::default(),
                effects: vec![],
                procedures: vec![],
                references: vec![],
            },
            procedure: Procedure {
                id: "proc-1".to_string(),
                command: command.to_string(),
                tool: None,
                is_local_command: None,
            },
            args: HashMap::new(),
            target_id: target_id.to_string(),
            exec_system_id: exec_system_id.to_string(),
        }
    }
}
