mod sys;
mod k8s;
mod iam;
mod network;

use std::collections::HashMap;
use std::sync::OnceLock;

use c2::{ExecTtp, TtpExecuted};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::external_parser::SystemFieldUpdates;
use crate::{Campaign, FactsUpdate};

pub const PARSER_VERSION: &str = "v1";
const RAW_PREVIEW_MAX_LEN: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParseResult {
    Parsed,
    KnownFailure,
    UnknownFormat,
    NoParser,
    ParserBug,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseAudit {
    pub cmd_id: String,
    pub effect_id: String,
    pub ttp_id: String,
    pub target_id: String,
    pub parser_version: String,
    pub raw_output_hash: String,
    pub raw_output_preview: String,
    pub parse_result: ParseResult,
    pub detail: String,
    pub inferred_facts_written: usize,
}

pub struct ParsedEffect {
    pub updates: FactsUpdate,
    pub audit: ParseAudit,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum ParserOutput {
    /// Parser updated system-level fields on the target entity.
    Success(SystemFieldUpdates, String),
    /// Parser produced new entities and relations (not tied to a system target).
    SuccessWithFacts(FactsUpdate, String),
    KnownFailure(String),
    UnknownFormat(String),
}

pub(crate) type ParserFn = fn(&str, &str) -> ParserOutput;

static REGISTRY: OnceLock<HashMap<&'static str, ParserFn>> = OnceLock::new();

fn get_registry() -> &'static HashMap<&'static str, ParserFn> {
    REGISTRY.get_or_init(|| {
        let mut m = HashMap::new();
        sys::register(&mut m);
        k8s::register(&mut m);
        iam::register(&mut m);
        network::register(&mut m);
        m
    })
}

pub fn parse_output_effect(
    campaign: &mut Campaign,
    effect_id: &str,
    cmd: &ExecTtp,
    event: &TtpExecuted,
) -> Option<ParsedEffect> {
    // Resolve the parser. Parametrized effects (e.g. `sys.has-binary(...)`) carry
    // their argument inside the effect-ID string; extract it and call the pure fn
    // directly, then fall through to the shared merge path below.
    let normalized = effect_id.trim().to_ascii_lowercase();

    let stdout = match event.results.first() {
        Some(s) => s.as_str(),
        None => {
            // sys.has-binary with a literal path encodes everything in the effect ID itself;
            // no stdout is needed. Let it fall through so the parser can record the binary.
            if normalized.starts_with("sys.has-binary(") {
                let inner = sys::extract_effect_args(effect_id).unwrap_or("");
                let is_output = inner.eq_ignore_ascii_case("${output}")
                    || inner.eq_ignore_ascii_case("output");
                if !is_output {
                    "" // literal path — proceed without stdout
                } else {
                    return Some(ParsedEffect {
                        updates: FactsUpdate::default(),
                        audit: build_audit(
                            effect_id,
                            cmd,
                            event,
                            ParseResult::KnownFailure,
                            "missing stdout payload",
                            0,
                        ),
                    });
                }
            } else {
                // Only return early if there is actually a registered/known parser.
                let is_known = normalized.starts_with("sys.hasfile(")
                    || normalized == "nmap"
                    || normalized == "k8s.selfsubjectrulesreview"
                    || get_registry().contains_key(normalized.trim());
                if !is_known {
                    return None;
                }
                return Some(ParsedEffect {
                    updates: FactsUpdate::default(),
                    audit: build_audit(
                        effect_id,
                        cmd,
                        event,
                        ParseResult::KnownFailure,
                        "missing stdout payload",
                        0,
                    ),
                });
            }
        }
    };
    let stderr = event.results.get(1).map(String::as_str).unwrap_or("");

    let parser_output: ParserOutput = if normalized.starts_with("sys.has-binary(") {
        let inner = sys::extract_effect_args(effect_id).unwrap_or("");
        sys::parse_sys_has_binary(stdout, inner)
    } else if normalized.starts_with("sys.hasfile(") {
        let path = sys::extract_effect_args(effect_id).unwrap_or("");
        sys::parse_sys_hasfile(stdout, path)
    } else if normalized == "nmap" {
        let source_id = cmd
            .args
            .get("TARGET_ID")
            .map(String::as_str)
            .unwrap_or(&cmd.target_id);
        network::parse_nmap(stdout, source_id)
    } else if normalized == "k8s.selfsubjectrulesreview" {
        let token_arg = cmd.args.get("TOKEN").map(String::as_str).unwrap_or("");
        // When an exec system is selected, cmd.target_id is rewritten to the pod ID.
        // TARGET_ID always holds the original logical target (SA entity) from the request.
        let fallback_target = cmd
            .args
            .get("TARGET_ID")
            .map(String::as_str)
            .unwrap_or(&cmd.target_id);
        iam::parse_self_subject_rules_review(stdout, stderr, fallback_target, token_arg)
    } else {
        let parser = get_registry().get(normalized.trim())?;
        parser(stdout, stderr)
    };

    match parser_output {
        ParserOutput::KnownFailure(detail) => Some(ParsedEffect {
            updates: FactsUpdate::default(),
            audit: build_audit(effect_id, cmd, event, ParseResult::KnownFailure, &detail, 0),
        }),
        ParserOutput::UnknownFormat(detail) => Some(ParsedEffect {
            updates: FactsUpdate::default(),
            audit: build_audit(effect_id, cmd, event, ParseResult::UnknownFormat, &detail, 0),
        }),
        ParserOutput::SuccessWithFacts(facts, detail) => {
            let facts_written = facts.new_entities.len() + facts.new_relations.len();
            Some(ParsedEffect {
                updates: facts,
                audit: build_audit(
                    effect_id,
                    cmd,
                    event,
                    ParseResult::Parsed,
                    &detail,
                    facts_written,
                ),
            })
        }
        ParserOutput::Success(updates, detail) => {
            let target_id = resolve_target_id(campaign, cmd);
            let Some(target_id) = target_id else {
                return Some(ParsedEffect {
                    updates: FactsUpdate::default(),
                    audit: build_audit(
                        effect_id,
                        cmd,
                        event,
                        ParseResult::KnownFailure,
                        "target is not a system entity (checked target_id and exec_system_id)",
                        0,
                    ),
                });
            };
            let facts_written = campaign
                .apply_system_update(&target_id, &updates)
                .unwrap_or(0);
            Some(ParsedEffect {
                updates: FactsUpdate::default(),
                audit: build_audit(
                    effect_id,
                    cmd,
                    event,
                    ParseResult::Parsed,
                    &detail,
                    facts_written,
                ),
            })
        }
    }
}

pub fn build_no_parser_audit(effect_id: &str, cmd: &ExecTtp, event: &TtpExecuted) -> ParseAudit {
    build_parse_audit(
        effect_id,
        cmd,
        event,
        ParseResult::NoParser,
        "no parser registered for effect",
        0,
    )
}

pub fn build_parse_audit(
    effect_id: &str,
    cmd: &ExecTtp,
    event: &TtpExecuted,
    parse_result: ParseResult,
    detail: &str,
    inferred_facts_written: usize,
) -> ParseAudit {
    build_audit(
        effect_id,
        cmd,
        event,
        parse_result,
        detail,
        inferred_facts_written,
    )
}

fn resolve_target_id(campaign: &Campaign, cmd: &ExecTtp) -> Option<String> {
    if campaign.get_system_entity(&cmd.target_id).is_some() {
        return Some(cmd.target_id.clone());
    }
    if !cmd.exec_system_id.trim().is_empty()
        && campaign.get_system_entity(&cmd.exec_system_id).is_some()
    {
        return Some(cmd.exec_system_id.clone());
    }
    None
}

fn build_audit(
    effect_id: &str,
    cmd: &ExecTtp,
    event: &TtpExecuted,
    parse_result: ParseResult,
    detail: &str,
    inferred_facts_written: usize,
) -> ParseAudit {
    ParseAudit {
        cmd_id: cmd.id.clone(),
        effect_id: effect_id.to_string(),
        ttp_id: cmd.ttp.id.clone(),
        target_id: cmd.target_id.clone(),
        parser_version: PARSER_VERSION.to_string(),
        raw_output_hash: hash_results(&event.results),
        raw_output_preview: truncate_preview(&join_results(&event.results)),
        parse_result,
        detail: detail.to_string(),
        inferred_facts_written,
    }
}

fn join_results(results: &[String]) -> String {
    if results.is_empty() {
        String::new()
    } else {
        results.join("\n---stderr---\n")
    }
}

fn hash_results(results: &[String]) -> String {
    let mut hasher = Sha256::new();
    for result in results {
        hasher.update(result.as_bytes());
        hasher.update([0x1e]);
    }

    format!("{:x}", hasher.finalize())
}

fn truncate_preview(payload: &str) -> String {
    if payload.len() <= RAW_PREVIEW_MAX_LEN {
        return payload.to_string();
    }

    format!("{}...", &payload[..RAW_PREVIEW_MAX_LEN])
}

#[cfg(test)]
mod tests {
    use super::*;
    use armory::{Procedure, Ttp};
    use ran_domain::{AccessLevel, Entity, Pod};
    use c2::ExecTtp;
    use std::collections::HashMap;

    fn sample_cmd() -> ExecTtp {
        ExecTtp {
            id: "cmd-1".to_string(),
            ttp: Ttp {
                id: "dummy-read-env".to_string(),
                name: "Read Env".to_string(),
                description: String::new(),
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
            started_at_ms: 0,
        }
    }

    fn sample_event(results: Vec<String>) -> TtpExecuted {
        TtpExecuted {
            id: "evt-1".to_string(),
            success: true,
            results,
            exit_code: 0,
            fail_reason: String::new(),
        }
    }

    #[test]
    fn parse_output_effect_returns_unknown_when_fixture_is_malformed() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let cmd = sample_cmd();
        let event = sample_event(vec!["not-an-env-line\njusttext".to_string()]);

        let parsed = parse_output_effect(&mut campaign, "sys.envvar", &cmd, &event).unwrap();

        assert!(matches!(parsed.audit.parse_result, ParseResult::UnknownFormat));
    }

    #[test]
    fn parse_output_effect_registry_lookup_is_case_insensitive() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.entities.insert_typed(pod);

        let cmd = sample_cmd();
        let event = sample_event(vec!["HOME=/root\nPATH=/usr/bin".to_string()]);

        let parsed = parse_output_effect(&mut campaign, "sys.envVar", &cmd, &event)
            .expect("mixed-case effect should resolve parser");

        assert!(matches!(parsed.audit.parse_result, ParseResult::Parsed));
    }

    #[test]
    fn parse_output_effect_still_parses_when_stderr_is_present() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.entities.insert_typed(pod);

        let cmd = sample_cmd();
        let event = sample_event(vec![
            "HOME=/root\nPATH=/usr/bin".to_string(),
            "warning: something noisy".to_string(),
        ]);

        let parsed = parse_output_effect(&mut campaign, "sys.envvar", &cmd, &event).unwrap();

        assert!(matches!(parsed.audit.parse_result, ParseResult::Parsed));
        assert!(parsed
            .audit
            .detail
            .contains("stderr had non-fatal content"));
    }

    #[test]
    fn parse_output_effect_falls_back_to_exec_system_id_for_updates() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.entities.insert_typed(pod);

        let mut cmd = sample_cmd();
        cmd.target_id = "sa/default/demo".to_string();
        cmd.exec_system_id = "ns/default/pod/demo".to_string();

        let event = sample_event(vec!["HOME=/root\nPATH=/usr/bin".to_string()]);

        let parsed = parse_output_effect(&mut campaign, "sys.envvar", &cmd, &event).unwrap();

        assert!(matches!(parsed.audit.parse_result, ParseResult::Parsed));

        let sys = campaign
            .get_system_entity("ns/default/pod/demo")
            .expect("pod should exist")
            .entity()
            .system();
        assert_eq!(sys.env_vars.get("HOME"), Some(&"/root".to_string()));
        assert_eq!(sys.env_vars.get("PATH"), Some(&"/usr/bin".to_string()));
    }

    #[test]
    fn parse_output_effect_sys_ip_missing_stdout_returns_known_failure() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let mut cmd = sample_cmd();
        cmd.ttp.effects = vec!["sys.ip".to_string()];
        let event = sample_event(vec![]);

        let parsed = parse_output_effect(&mut campaign, "sys.ip", &cmd, &event).unwrap();

        assert!(matches!(parsed.audit.parse_result, ParseResult::KnownFailure));
    }

    #[test]
    fn parse_output_effect_sys_ip_malformed_returns_unknown_format() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let mut cmd = sample_cmd();
        cmd.ttp.effects = vec!["sys.ip".to_string()];
        let event = sample_event(vec!["not-an-ip-at-all".to_string()]);

        let parsed = parse_output_effect(&mut campaign, "sys.ip", &cmd, &event).unwrap();

        assert!(matches!(parsed.audit.parse_result, ParseResult::UnknownFormat));
    }

    #[test]
    fn parse_output_effect_sys_ip_writes_ips_to_entity() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.entities.insert_typed(pod);

        let mut cmd = sample_cmd();
        cmd.ttp.effects = vec!["sys.ip".to_string()];
        let event = sample_event(vec!["10.0.0.1 172.16.0.5".to_string()]);

        let parsed = parse_output_effect(&mut campaign, "sys.ip", &cmd, &event).unwrap();

        assert!(matches!(parsed.audit.parse_result, ParseResult::Parsed));

        let sys = campaign
            .get_system_entity("ns/default/pod/demo")
            .expect("pod should exist")
            .entity()
            .system();
        assert!(sys.ips.iter().any(|ip| ip.to_string() == "10.0.0.1"));
        assert!(sys.ips.iter().any(|ip| ip.to_string() == "172.16.0.5"));
    }

    #[test]
    fn parse_output_effect_sys_userid_writes_access_level_to_entity() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.entities.insert_typed(pod);

        let mut cmd = sample_cmd();
        cmd.ttp.effects = vec!["sys.userID".to_string()];
        let event = sample_event(vec!["uid=0(root) gid=0(root) groups=0(root)".to_string()]);

        let parsed = parse_output_effect(&mut campaign, "sys.userid", &cmd, &event).unwrap();
        assert!(matches!(parsed.audit.parse_result, ParseResult::Parsed));

        let sys = campaign
            .get_system_entity("ns/default/pod/demo")
            .expect("pod should exist")
            .entity()
            .system();
        assert_eq!(sys.user_id, Some(0));
        assert_eq!(sys.username.as_deref(), Some("root"));
        assert_eq!(sys.access_level, AccessLevel::Exec);
    }

    #[test]
    fn parse_output_effect_linux_mounts_writes_mounts_to_entity() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.entities.insert_typed(pod);

        let mut cmd = sample_cmd();
        cmd.ttp.effects = vec!["linux.mounts".to_string()];
        let stdout = "sysfs on /sys type sysfs (rw)\n/dev/sda1 on / type ext4 (rw)\n".to_string();
        let event = sample_event(vec![stdout]);

        let parsed = parse_output_effect(&mut campaign, "linux.mounts", &cmd, &event).unwrap();
        assert!(matches!(parsed.audit.parse_result, ParseResult::Parsed));

        let sys = campaign
            .get_system_entity("ns/default/pod/demo")
            .expect("pod should exist")
            .entity()
            .system();
        assert_eq!(sys.mounts.len(), 2);
    }

    #[test]
    fn parse_output_effect_sys_processes_writes_to_entity() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.entities.insert_typed(pod);

        let mut cmd = sample_cmd();
        cmd.ttp.effects = vec!["sys.processes".to_string()];
        let stdout = "USER PID PPID CPU START TTY TIME CMD\n\
                      root 1 0 0.0 00:00 ? 0:00 /sbin/init\n"
            .to_string();
        let event = sample_event(vec![stdout]);

        let parsed = parse_output_effect(&mut campaign, "sys.processes", &cmd, &event).unwrap();
        assert!(matches!(parsed.audit.parse_result, ParseResult::Parsed));

        let sys = campaign
            .get_system_entity("ns/default/pod/demo")
            .expect("pod should exist")
            .entity()
            .system();
        assert_eq!(sys.processes.len(), 1);
        assert_eq!(sys.processes[0].pid, 1);
        assert_eq!(sys.processes[0].user, Some("root".to_string()));
    }

    #[test]
    fn parse_output_effect_has_binary_writes_to_entity() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.entities.insert_typed(pod);

        let mut cmd = sample_cmd();
        cmd.ttp.effects = vec!["sys.has-binary(/usr/bin/curl)".to_string()];
        let event = sample_event(vec![String::new()]);

        // effect_id is the (already ground-template-substituted) string
        let parsed = parse_output_effect(&mut campaign, "sys.has-binary(/usr/bin/curl)", &cmd, &event).unwrap();
        assert!(matches!(parsed.audit.parse_result, ParseResult::Parsed));

        let sys = campaign
            .get_system_entity("ns/default/pod/demo")
            .expect("pod should exist")
            .entity()
            .system();
        use ran_domain::BinaryPresence;
        assert_eq!(sys.has_binary("curl"), BinaryPresence::Present("/usr/bin/curl".to_string()));
    }

    #[test]
    fn parse_output_effect_has_binary_literal_path_no_stdout_still_parses() {
        // Regression: sys.has-binary(/tmp/ran-ws) with empty results should NOT produce
        // KnownFailure("missing stdout payload") — the path is in the effect ID itself.
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.entities.insert_typed(pod);

        let mut cmd = sample_cmd();
        cmd.ttp.effects = vec!["sys.has-binary(/tmp/ran-ws)".to_string()];
        // No stdout at all — empty results vec
        let event = sample_event(vec![]);

        let parsed = parse_output_effect(&mut campaign, "sys.has-binary(/tmp/ran-ws)", &cmd, &event).unwrap();
        assert!(
            matches!(parsed.audit.parse_result, ParseResult::Parsed),
            "expected Parsed, got {:?}: {}",
            parsed.audit.parse_result,
            parsed.audit.detail
        );

        let sys = campaign
            .get_system_entity("ns/default/pod/demo")
            .expect("pod should exist")
            .entity()
            .system();
        use ran_domain::BinaryPresence;
        assert_eq!(sys.has_binary("ran-ws"), BinaryPresence::Present("/tmp/ran-ws".to_string()));
    }
}
