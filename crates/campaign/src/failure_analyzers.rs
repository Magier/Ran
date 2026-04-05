use c2::{ExecTtp, TtpExecuted};

use crate::ParseResult;

pub const FAILURE_ANALYZER_EFFECT_ID: &str = "execution.failure";

#[derive(Debug, Clone)]
pub struct FailureClassification {
    pub parse_result: ParseResult,
    pub detail: String,
}

pub trait FailureAnalyzer: Send + Sync {
    fn analyze(&self, cmd: &ExecTtp, event: &TtpExecuted) -> Option<FailureClassification>;
}

pub struct InvalidTargetFailureAnalyzer;
pub struct RbacDeniedFailureAnalyzer;
pub struct ConnectivityFailureAnalyzer;
pub struct CommandNotFoundFailureAnalyzer;

impl FailureAnalyzer for InvalidTargetFailureAnalyzer {
    fn analyze(&self, _cmd: &ExecTtp, event: &TtpExecuted) -> Option<FailureClassification> {
        if contains_any(&event.fail_reason, &["invalid pod target id"]) {
            return Some(FailureClassification {
                parse_result: ParseResult::KnownFailure,
                detail: "invalid target identifier for pod exec".to_string(),
            });
        }

        None
    }
}

impl FailureAnalyzer for RbacDeniedFailureAnalyzer {
    fn analyze(&self, _cmd: &ExecTtp, event: &TtpExecuted) -> Option<FailureClassification> {
        let haystack = failure_haystack(event);
        if contains_any(
            &haystack,
            &[
                "forbidden",
                "permission denied",
                "access denied",
                "cannot",
                "is forbidden",
            ],
        ) {
            return Some(FailureClassification {
                parse_result: ParseResult::KnownFailure,
                detail: "access denied by RBAC or runtime policy".to_string(),
            });
        }

        None
    }
}

impl FailureAnalyzer for ConnectivityFailureAnalyzer {
    fn analyze(&self, _cmd: &ExecTtp, event: &TtpExecuted) -> Option<FailureClassification> {
        let haystack = failure_haystack(event);
        if contains_any(
            &haystack,
            &[
                "connection refused",
                "no route to host",
                "i/o timeout",
                "timed out",
                "context deadline exceeded",
                "temporary failure in name resolution",
            ],
        ) {
            return Some(FailureClassification {
                parse_result: ParseResult::KnownFailure,
                detail: "network connectivity failure while executing procedure".to_string(),
            });
        }

        None
    }
}

impl FailureAnalyzer for CommandNotFoundFailureAnalyzer {
    fn analyze(&self, _cmd: &ExecTtp, event: &TtpExecuted) -> Option<FailureClassification> {
        // Exit code 127 is the POSIX shell standard for "command not found".
        if event.exit_code == 127 {
            return Some(FailureClassification {
                parse_result: ParseResult::KnownFailure,
                detail: "command or binary was not found in execution environment".to_string(),
            });
        }

        let haystack = failure_haystack(event);
        if contains_any(
            &haystack,
            &[
                "command not found",
                "executable file not found",
                "not found in $path",
                // kube exec embeds the exit code in fail_reason as a string
                // even when event.exit_code is not propagated correctly
                "exit code 127",
            ],
        ) {
            return Some(FailureClassification {
                parse_result: ParseResult::KnownFailure,
                detail: "command or binary was not found in execution environment".to_string(),
            });
        }

        None
    }
}

pub fn default_failure_analyzers() -> Vec<Box<dyn FailureAnalyzer>> {
    vec![
        Box::new(InvalidTargetFailureAnalyzer),
        Box::new(RbacDeniedFailureAnalyzer),
        Box::new(ConnectivityFailureAnalyzer),
        Box::new(CommandNotFoundFailureAnalyzer),
    ]
}

pub fn classify_failure(cmd: &ExecTtp, event: &TtpExecuted) -> FailureClassification {
    for analyzer in default_failure_analyzers() {
        if let Some(classified) = analyzer.analyze(cmd, event) {
            return classified;
        }
    }

    let detail = if event.fail_reason.trim().is_empty() {
        format!(
            "execution failed with unclassified reason (exit code {})",
            event.exit_code
        )
    } else {
        format!("unclassified failure: {}", event.fail_reason.trim())
    };

    FailureClassification {
        parse_result: ParseResult::UnknownFormat,
        detail,
    }
}

fn failure_haystack(event: &TtpExecuted) -> String {
    if event.results.is_empty() {
        event.fail_reason.clone()
    } else {
        format!("{}\n{}", event.fail_reason, event.results.join("\n"))
    }
}

fn contains_any(input: &str, needles: &[&str]) -> bool {
    let normalized = input.to_ascii_lowercase();
    needles.iter().any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use armory::{Procedure, Ttp};

    use super::*;

    fn sample_cmd() -> ExecTtp {
        ExecTtp {
            id: "cmd-1".to_string(),
            ttp: Ttp {
                id: "ttp-test".to_string(),
                name: "Test TTP".to_string(),
                description: "test".to_string(),
                tactic: "Discovery".to_string(),
                techniques: vec![],
                status: "stable".to_string(),
                params: vec![],
                requires: Default::default(),
                effects: vec!["sys.envvar".to_string()],
                procedures: vec![Procedure {
                    id: "shell".to_string(),
                    command: "env".to_string(),
                    tool: None,
                    is_local_command: None,
                }],
                references: vec![],
            },
            procedure: Procedure {
                id: "shell".to_string(),
                command: "env".to_string(),
                tool: None,
                is_local_command: None,
            },
            args: HashMap::new(),
            target_id: "ns/default/pod/demo".to_string(),
            exec_system_id: String::new(),
        }
    }

    fn sample_failed_event(fail_reason: &str, stderr: &str) -> TtpExecuted {
        TtpExecuted {
            id: "evt-1".to_string(),
            success: false,
            results: vec![stderr.to_string()],
            exit_code: 1,
            fail_reason: fail_reason.to_string(),
        }
    }

    #[test]
    fn classify_failure_detects_known_rbac_denial() {
        let cmd = sample_cmd();
        let event = sample_failed_event("Forbidden", "Error from server (Forbidden)");

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.detail.contains("RBAC"));
    }

    #[test]
    fn classify_failure_detects_exit_code_127_as_command_not_found() {
        let cmd = sample_cmd();
        let mut event = sample_failed_event(
            "command terminated with non-zero exit code: error executing command [/bin/sh -lc ps -ef], exit code 127",
            "",
        );
        event.exit_code = 127;

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.detail.contains("not found"));
    }

    #[test]
    fn classify_failure_detects_exit_code_127_in_fail_reason_string() {
        // kube exec embeds exit code in fail_reason but may not set event.exit_code
        let cmd = sample_cmd();
        let event = sample_failed_event(
            "command terminated with non-zero exit code: error executing command [/bin/sh -lc nmap -sT -sV -F 10.244.1.2/24], exit code 127",
            "",
        );
        // exit_code is left at 1 (not 127) to simulate the propagation gap

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.detail.contains("not found"));
    }

    #[test]
    fn classify_failure_detects_unknown_failure() {
        let cmd = sample_cmd();
        let event = sample_failed_event("mystery runtime issue", "");

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::UnknownFormat));
        assert!(classified.detail.contains("unclassified"));
    }
}
