use c2::{ExecTtp, TtpExecuted};

use crate::ParseResult;

pub const FAILURE_ANALYZER_EFFECT_ID: &str = "execution.failure";

#[derive(Debug, Clone)]
pub struct FailureClassification {
    pub parse_result: ParseResult,
    pub detail: String,
    /// The binary that was not found, extracted from the error output.
    /// When present this is more reliable than the procedure's declared tool name.
    pub extracted_binary: Option<String>,
    /// True when the failure was classified as a missing binary / command not found.
    pub is_binary_missing: bool,
}

impl FailureClassification {
    fn known_failure(detail: impl Into<String>) -> Self {
        FailureClassification {
            parse_result: ParseResult::KnownFailure,
            detail: detail.into(),
            extracted_binary: None,
            is_binary_missing: false,
        }
    }

    fn unknown_format(detail: impl Into<String>) -> Self {
        FailureClassification {
            parse_result: ParseResult::UnknownFormat,
            detail: detail.into(),
            extracted_binary: None,
            is_binary_missing: false,
        }
    }

    fn binary_missing(binary: Option<String>) -> Self {
        let detail = match &binary {
            Some(name) => format!("binary '{}' was not found on the system", name),
            None => "command or binary was not found in execution environment".to_string(),
        };
        FailureClassification {
            parse_result: ParseResult::KnownFailure,
            detail,
            extracted_binary: binary,
            is_binary_missing: true,
        }
    }
}

pub trait FailureAnalyzer: Send + Sync {
    fn analyze(&self, cmd: &ExecTtp, event: &TtpExecuted) -> Option<FailureClassification>;
}

pub struct InvalidTargetFailureAnalyzer;
pub struct RbacDeniedFailureAnalyzer;
pub struct ConnectivityFailureAnalyzer;
pub struct CommandNotFoundFailureAnalyzer;
pub struct NotWriteableFailureAnalyzer;

impl FailureAnalyzer for InvalidTargetFailureAnalyzer {
    fn analyze(&self, _cmd: &ExecTtp, event: &TtpExecuted) -> Option<FailureClassification> {
        if contains_any(&event.fail_reason, &["invalid pod target id"]) {
            return Some(FailureClassification::known_failure(
                "invalid target identifier for pod exec",
            ));
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
            return Some(FailureClassification::known_failure(
                "access denied by RBAC or runtime policy",
            ));
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
            return Some(FailureClassification::known_failure(
                "network connectivity failure while executing procedure",
            ));
        }

        None
    }
}

impl FailureAnalyzer for NotWriteableFailureAnalyzer {
    fn analyze(&self, _cmd: &ExecTtp, event: &TtpExecuted) -> Option<FailureClassification> {
        // Exit code 23 is used by tool-transfer TTPs to signal a non-writable destination.
        if event.exit_code == 23 {
            return Some(FailureClassification::known_failure(
                "destination directory is not writable",
            ));
        }

        let haystack = failure_haystack(event);
        if contains_any(&haystack, &["is not writeable", "exit code 23"]) {
            return Some(FailureClassification::known_failure(
                "destination directory is not writable",
            ));
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Binary-not-found template dictionary
// ---------------------------------------------------------------------------

/// Describes one recognizable "binary not found" error format.
///
/// Templates are checked in order; the first whose `hint` substring is present
/// in the (lowercased) error haystack is used. When an `extract` fn is
/// provided it attempts to pull the missing binary's name out of the raw
/// (non-lowercased) haystack.
///
/// Adding a new shell/runtime format is as simple as appending an entry here —
/// no other code needs to change.
pub struct BinaryNotFoundTemplate {
    /// Human-readable label used in diagnostics / tests.
    pub description: &'static str,
    /// Lowercase substring that must be present before extraction is tried.
    pub hint: &'static str,
    /// Optionally extract the missing binary name from the full error text.
    pub extract: Option<fn(&str) -> Option<String>>,
}

/// All recognised "binary not found" error formats, in precedence order.
///
/// Formats covered:
/// | Shell / runtime | Example |
/// |---|---|
/// | POSIX sh / dash / busybox | `/bin/sh: kubectl: not found` |
/// | POSIX sh (numbered) | `sh: 1: kubectl: not found` |
/// | bash | `bash: kubectl: command not found` |
/// | zsh | `zsh: command not found: kubectl` |
/// | fish | `fish: Unknown command: kubectl` |
/// | K8s OCI exec | `exec: "kubectl": executable file not found in $PATH` |
/// | K8s OCI exec | `exec: "kubectl": no such file or directory` |
/// | K8s fail_reason | `… exit code 127` (embedded string) |
/// | Generic PATH | `not found in $PATH` |
pub static BINARY_NOT_FOUND_TEMPLATES: &[BinaryNotFoundTemplate] = &[
    // zsh puts the binary name AFTER the marker — must be checked before the
    // bash template because "zsh: command not found: name" also contains the
    // bash hint ": command not found".
    //   "zsh: command not found: kubectl"
    BinaryNotFoundTemplate {
        description: "zsh — 'command not found: <name>'",
        hint: "command not found: ",
        extract: Some(|h| extract_after_marker(h, "command not found: ")),
    },
    // fish shell:
    //   "fish: Unknown command: kubectl"
    BinaryNotFoundTemplate {
        description: "fish — 'Unknown command: <name>'",
        hint: "unknown command: ",
        extract: Some(|h| extract_after_marker(h, "unknown command: ")),
    },
    // POSIX sh / dash / busybox ash:
    //   "/bin/sh: kubectl: not found"
    //   "sh: 1: kubectl: not found"          (numbered line form)
    //   "/usr/bin/sh: 1: curl: not found"
    BinaryNotFoundTemplate {
        description: "POSIX sh — '<name>: not found'",
        hint: ": not found",
        extract: Some(|h| extract_before_suffix(h, ": not found")),
    },
    // bash / sh command-not-found handler:
    //   "bash: kubectl: command not found"
    //   "/bin/bash: wget: command not found"
    BinaryNotFoundTemplate {
        description: "bash — '<name>: command not found'",
        hint: ": command not found",
        extract: Some(|h| extract_before_suffix(h, ": command not found")),
    },
    // K8s OCI / containerd / runc — binary not in $PATH:
    //   "exec: \"kubectl\": executable file not found in $PATH"
    BinaryNotFoundTemplate {
        description: "OCI — exec: \"<name>\": executable file not found in $PATH",
        hint: "executable file not found",
        extract: Some(extract_quoted_exec_name),
    },
    // K8s OCI / execve — binary path does not exist:
    //   "exec: \"kubectl\": no such file or directory"
    //
    // Note: the hint `"exec: \""` is intentionally narrow so that file-not-found
    // errors for config files (e.g. `open /etc/config: no such file or directory`)
    // do not false-positive here.
    BinaryNotFoundTemplate {
        description: "OCI — exec: \"<name>\": no such file or directory",
        hint: "exec: \"",
        extract: Some(extract_quoted_exec_name),
    },
    // K8s wraps the exit code in fail_reason as a string even when
    // event.exit_code is not propagated correctly:
    //   "command terminated with non-zero exit code: …, exit code 127"
    BinaryNotFoundTemplate {
        description: "embedded exit code 127 in fail_reason",
        hint: "exit code 127",
        extract: None,
    },
    // Generic PATH search failure (no binary name extractable):
    BinaryNotFoundTemplate {
        description: "generic — 'not found in $PATH'",
        hint: "not found in $path",
        extract: None,
    },
];

impl FailureAnalyzer for CommandNotFoundFailureAnalyzer {
    fn analyze(&self, _cmd: &ExecTtp, event: &TtpExecuted) -> Option<FailureClassification> {
        let haystack = failure_haystack(event);
        let haystack_lower = haystack.to_ascii_lowercase();

        let template_matched = BINARY_NOT_FOUND_TEMPLATES
            .iter()
            .any(|t| haystack_lower.contains(t.hint));

        if event.exit_code != 127 && !template_matched {
            return None;
        }

        // Try to extract the binary name from the first matching template that
        // provides an extract fn.
        let extracted = BINARY_NOT_FOUND_TEMPLATES
            .iter()
            .filter(|t| haystack_lower.contains(t.hint))
            .filter_map(|t| t.extract.and_then(|f| f(&haystack)))
            .next();

        Some(FailureClassification::binary_missing(extracted))
    }
}

pub fn default_failure_analyzers() -> Vec<Box<dyn FailureAnalyzer>> {
    vec![
        Box::new(InvalidTargetFailureAnalyzer),
        Box::new(RbacDeniedFailureAnalyzer),
        Box::new(ConnectivityFailureAnalyzer),
        Box::new(CommandNotFoundFailureAnalyzer),
        Box::new(NotWriteableFailureAnalyzer),
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

    FailureClassification::unknown_format(detail)
}

// ---------------------------------------------------------------------------
// Name extraction helpers
// ---------------------------------------------------------------------------

/// Extract a binary name from patterns where the name appears immediately before
/// the given suffix on a line.
///
/// Handles POSIX sh and bash formats:
/// - `"/bin/sh: kubectl: not found"` → `"kubectl"` (suffix `": not found"`)
/// - `"sh: 1: kubectl: not found"` → `"kubectl"` (skips numeric token `1`)
/// - `"bash: kubectl: command not found"` → `"kubectl"` (suffix `": command not found"`)
fn extract_before_suffix(haystack: &str, suffix: &str) -> Option<String> {
    let suffix_lower = suffix.to_ascii_lowercase();
    for line in haystack.lines() {
        let line_lower = line.to_ascii_lowercase();
        if let Some(end) = line_lower.find(&suffix_lower) {
            let before = line[..end].trim();
            if let Some(name) = last_non_digit_colon_segment(before) {
                return Some(name);
            }
        }
    }
    None
}

/// Extract a binary name from patterns where the name appears immediately AFTER
/// the given marker on a line.
///
/// Handles:
/// - zsh: `"zsh: command not found: kubectl"` (marker `"command not found: "`)
/// - fish: `"fish: Unknown command: kubectl"` (marker `"unknown command: "`)
fn extract_after_marker(haystack: &str, marker: &str) -> Option<String> {
    let marker_lower = marker.to_ascii_lowercase();
    for line in haystack.lines() {
        if let Some(pos) = line.to_ascii_lowercase().find(&marker_lower) {
            let after = line[pos + marker.len()..].trim();
            if let Some(name) = after.split_whitespace().next() {
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

/// Extract a binary name from OCI/execve formats that quote the name:
/// - `exec: "kubectl": executable file not found in $PATH`
/// - `exec: "kubectl": no such file or directory`
fn extract_quoted_exec_name(haystack: &str) -> Option<String> {
    for line in haystack.lines() {
        if let Some(pos) = line.find("exec: \"") {
            let start = pos + 7; // len(r#"exec: ""#)
            if let Some(end_rel) = line[start..].find('"') {
                let name = &line[start..start + end_rel];
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

/// Walk backwards through `": "`-delimited segments and return the rightmost
/// one that is neither empty nor purely numeric.
///
/// This handles:
/// - `/bin/sh: kubectl`       → `"kubectl"`  (skips the shell path)
/// - `sh: 1: kubectl`         → `"kubectl"`  (skips the numeric line number)
/// - `/usr/bin/sh: 1: curl`   → `"curl"`
///
/// When a segment contains a `/` its basename is returned so that a shell path
/// like `/bin/sh` yields `"sh"` rather than the full path — though in practice
/// the shell segment is never the final one in a real "binary not found" message.
fn last_non_digit_colon_segment(s: &str) -> Option<String> {
    let mut slice = s;
    loop {
        let (segment, rest) = if let Some(pos) = slice.rfind(": ") {
            (&slice[pos + 2..], &slice[..pos])
        } else {
            (slice, "")
        };

        let trimmed = segment.trim();
        if !trimmed.is_empty() && !trimmed.chars().all(|c| c.is_ascii_digit()) {
            let name = trimmed.rsplit('/').next().unwrap_or(trimmed);
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }

        if rest.is_empty() {
            return None;
        }
        slice = rest;
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
            exec_chain: vec!["ns/default/pod/demo".to_string()],
            exec_system_id: String::new(),
            started_at_ms: 0,
            output_transform: None,
        }
    }

    fn failed_event_stderr(stderr: &str) -> TtpExecuted {
        TtpExecuted {
            id: "evt-1".to_string(),
            success: false,
            results: vec![stderr.to_string()],
            exit_code: 1,
            fail_reason: String::new(),
        }
    }

    fn failed_event_fail_reason(fail_reason: &str) -> TtpExecuted {
        TtpExecuted {
            id: "evt-1".to_string(),
            success: false,
            results: vec![],
            exit_code: 1,
            fail_reason: fail_reason.to_string(),
        }
    }

    // --- previously-existing tests (must keep passing) ---

    #[test]
    fn classify_failure_detects_known_rbac_denial() {
        let cmd = sample_cmd();
        let event = TtpExecuted {
            id: "evt-1".to_string(),
            success: false,
            results: vec!["Error from server (Forbidden)".to_string()],
            exit_code: 1,
            fail_reason: "Forbidden".to_string(),
        };

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.detail.contains("RBAC"));
    }

    #[test]
    fn classify_failure_detects_exit_code_127_as_command_not_found() {
        let cmd = sample_cmd();
        let mut event = failed_event_fail_reason(
            "command terminated with non-zero exit code: error executing command [/bin/sh -lc ps -ef], exit code 127",
        );
        event.exit_code = 127;

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.is_binary_missing);
    }

    #[test]
    fn classify_failure_detects_exit_code_127_in_fail_reason_string() {
        // kube exec embeds exit code in fail_reason but may not set event.exit_code
        let cmd = sample_cmd();
        let event = failed_event_fail_reason(
            "command terminated with non-zero exit code: error executing command [/bin/sh -lc nmap -sT -sV -F 10.244.1.2/24], exit code 127",
        );
        // exit_code is left at 1 (not 127) to simulate the propagation gap

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.is_binary_missing);
    }

    #[test]
    fn classify_failure_detects_not_writeable_via_exit_code() {
        let cmd = sample_cmd();
        let mut event = TtpExecuted {
            id: "evt-1".to_string(),
            success: false,
            results: vec!["'/usr/local/bin' is not writeable".to_string()],
            exit_code: 23,
            fail_reason: "command terminated with non-zero exit code: error executing command [/bin/sh -lc ...], exit code 23".to_string(),
        };
        event.exit_code = 23;

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.detail.contains("not writable"));
    }

    #[test]
    fn classify_failure_detects_not_writeable_via_message() {
        let cmd = sample_cmd();
        let event = failed_event_fail_reason(
            "command terminated with non-zero exit code: error executing command [...], exit code 23",
        );

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.detail.contains("not writable"));
    }

    #[test]
    fn classify_failure_detects_unknown_failure() {
        let cmd = sample_cmd();
        let event = failed_event_fail_reason("mystery runtime issue");

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(
            classified.parse_result,
            ParseResult::UnknownFormat
        ));
        assert!(classified.detail.contains("unclassified"));
    }

    // --- new tests covering all BINARY_NOT_FOUND_TEMPLATES ---

    /// Go test: `TestAnalyzeFailedTTPExecution_ToolNotFound` case 1
    /// Input: `"command terminated with exit code 127: 'sh: 1: kubectl: not found\n'"`
    #[test]
    fn posix_sh_numbered_kubectl_not_found_extracts_name() {
        let cmd = sample_cmd();
        let event = failed_event_fail_reason(
            "command terminated with exit code 127: 'sh: 1: kubectl: not found\n'",
        );

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.is_binary_missing);
        assert_eq!(classified.extracted_binary.as_deref(), Some("kubectl"));
        assert!(classified.detail.contains("kubectl"));
    }

    /// Go test: `TestAnalyzeFailedTTPExecution_ToolNotFound` case 2
    /// Input: long OCI runtime error containing `exec: "kubectl": executable file not found in $PATH`
    #[test]
    fn oci_runtime_exec_not_found_extracts_name() {
        let cmd = sample_cmd();
        let event = failed_event_fail_reason(
            r#"error: Internal error occurred: error executing command in container: failed to exec in container: failed to start exec "arstarst123": OCI runtime exec failed: exec failed: unable to start container process: exec: "kubectl": executable file not found in $PATH"#,
        );

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.is_binary_missing);
        assert_eq!(classified.extracted_binary.as_deref(), Some("kubectl"));
    }

    /// Go test: `TestAnalyzeFailedTTP_BinaryNotFoundShouldUpdateBinariesOnExecutingSystem`
    /// Multiple result lines, one of which is `/usr/bin/sh: 1: curl: not found`
    #[test]
    fn posix_sh_curl_not_found_in_results_extracts_name() {
        let cmd = sample_cmd();
        let event = TtpExecuted {
            id: "evt-1".to_string(),
            success: false,
            results: vec![
                "Error 127\n".to_string(),
                "/usr/bin/sh: 1: curl: not found\n".to_string(),
                "command terminated with exit code 127: '/usr/bin/sh: 1: curl: not found\n'"
                    .to_string(),
            ],
            exit_code: 1,
            fail_reason: String::new(),
        };

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.is_binary_missing);
        assert_eq!(classified.extracted_binary.as_deref(), Some("curl"));
    }

    /// The exact error from the reported bug: `/bin/sh: kubectl: not found`
    #[test]
    fn posix_sh_kubectl_not_found_plain() {
        let cmd = sample_cmd();
        let event = failed_event_stderr("/bin/sh: kubectl: not found");

        let classified = classify_failure(&cmd, &event);

        assert!(matches!(classified.parse_result, ParseResult::KnownFailure));
        assert!(classified.is_binary_missing);
        assert_eq!(classified.extracted_binary.as_deref(), Some("kubectl"));
        assert!(classified.detail.contains("kubectl"));
    }

    #[test]
    fn bash_command_not_found_extracts_name() {
        let cmd = sample_cmd();
        let event = failed_event_stderr("bash: wget: command not found");

        let classified = classify_failure(&cmd, &event);

        assert!(classified.is_binary_missing);
        assert_eq!(classified.extracted_binary.as_deref(), Some("wget"));
    }

    #[test]
    fn zsh_command_not_found_extracts_name() {
        let cmd = sample_cmd();
        let event = failed_event_stderr("zsh: command not found: nmap");

        let classified = classify_failure(&cmd, &event);

        assert!(classified.is_binary_missing);
        assert_eq!(classified.extracted_binary.as_deref(), Some("nmap"));
    }

    #[test]
    fn fish_unknown_command_extracts_name() {
        let cmd = sample_cmd();
        let event = failed_event_stderr("fish: Unknown command: kubectl");

        let classified = classify_failure(&cmd, &event);

        assert!(classified.is_binary_missing);
        assert_eq!(classified.extracted_binary.as_deref(), Some("kubectl"));
    }

    #[test]
    fn oci_exec_no_such_file_extracts_name() {
        let cmd = sample_cmd();
        let event = failed_event_stderr(r#"exec: "my-tool": no such file or directory"#);

        let classified = classify_failure(&cmd, &event);

        assert!(classified.is_binary_missing);
        assert_eq!(classified.extracted_binary.as_deref(), Some("my-tool"));
    }

    #[test]
    fn exit_code_127_no_output_still_detected() {
        let cmd = sample_cmd();
        let mut event = TtpExecuted {
            id: "evt-1".to_string(),
            success: false,
            results: vec![],
            exit_code: 127,
            fail_reason: String::new(),
        };
        event.exit_code = 127;

        let classified = classify_failure(&cmd, &event);

        assert!(classified.is_binary_missing);
        assert_eq!(classified.extracted_binary, None);
    }

    #[test]
    fn not_found_in_path_generic_detected() {
        let cmd = sample_cmd();
        let event = failed_event_stderr("helm: not found in $PATH");

        let classified = classify_failure(&cmd, &event);

        assert!(classified.is_binary_missing);
        // "helm: not found in $PATH" matches the POSIX sh ": not found" template first
        // and helm gets extracted correctly
        assert_eq!(classified.extracted_binary.as_deref(), Some("helm"));
    }

    // --- extraction unit tests ---

    #[test]
    fn extract_before_suffix_handles_path_prefix_and_number() {
        assert_eq!(
            extract_before_suffix("/bin/sh: kubectl: not found", ": not found"),
            Some("kubectl".to_string())
        );
        assert_eq!(
            extract_before_suffix("sh: 1: kubectl: not found", ": not found"),
            Some("kubectl".to_string())
        );
        assert_eq!(
            extract_before_suffix("/usr/bin/sh: 1: curl: not found", ": not found"),
            Some("curl".to_string())
        );
    }

    #[test]
    fn extract_after_marker_zsh() {
        assert_eq!(
            extract_after_marker("zsh: command not found: nmap", "command not found: "),
            Some("nmap".to_string())
        );
    }

    #[test]
    fn extract_quoted_exec_name_from_oci_line() {
        assert_eq!(
            extract_quoted_exec_name(r#"exec: "kubectl": executable file not found in $PATH"#),
            Some("kubectl".to_string())
        );
        assert_eq!(
            extract_quoted_exec_name(r#"exec: "my-tool": no such file or directory"#),
            Some("my-tool".to_string())
        );
    }
}
