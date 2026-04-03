use std::collections::HashMap;
use std::net::IpAddr;

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
enum ParserOutput {
    Success(SystemFieldUpdates, String),
    KnownFailure(String),
    UnknownFormat(String),
}

type InternalParserFn = fn(stdout: &str, stderr: &str) -> ParserOutput;

pub fn parse_output_effect(
    campaign: &mut Campaign,
    effect_id: &str,
    cmd: &ExecTtp,
    event: &TtpExecuted,
) -> Option<ParsedEffect> {
    let parser = resolve_output_parser(effect_id)?;

    let stdout = match event.results.first() {
        Some(s) => s.as_str(),
        None => {
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
    };
    let stderr = event.results.get(1).map(String::as_str).unwrap_or("");

    match parser(stdout, stderr) {
        ParserOutput::KnownFailure(detail) => Some(ParsedEffect {
            updates: FactsUpdate::default(),
            audit: build_audit(effect_id, cmd, event, ParseResult::KnownFailure, &detail, 0),
        }),
        ParserOutput::UnknownFormat(detail) => Some(ParsedEffect {
            updates: FactsUpdate::default(),
            audit: build_audit(effect_id, cmd, event, ParseResult::UnknownFormat, &detail, 0),
        }),
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

fn resolve_output_parser(effect_name: &str) -> Option<InternalParserFn> {
    match effect_name.trim().to_ascii_lowercase().as_str() {
        "sys.envvar" => Some(parse_sys_envvar),
        "sys.ip" => Some(parse_sys_ip),
        _ => None,
    }
}

fn parse_sys_envvar(stdout: &str, stderr: &str) -> ParserOutput {
    let vars = parse_env_vars(stdout);
    if vars.is_empty() && !stdout.trim().is_empty() {
        return ParserOutput::UnknownFormat(
            "stdout did not contain parseable KEY=VALUE lines".to_string(),
        );
    }

    let detail = if stderr.trim().is_empty() {
        "parsed and merged environment variables".to_string()
    } else {
        "parsed and merged environment variables (stderr had non-fatal content)".to_string()
    };

    ParserOutput::Success(
        SystemFieldUpdates {
            env_vars: vars,
            ..Default::default()
        },
        detail,
    )
}

fn parse_sys_ip(stdout: &str, _stderr: &str) -> ParserOutput {
    let ips = parse_ip_addrs(stdout);
    if ips.is_empty() && !stdout.trim().is_empty() {
        return ParserOutput::UnknownFormat(
            "stdout did not contain parseable IP addresses".to_string(),
        );
    }

    let detail = format!("parsed {} IP address(es)", ips.len());
    ParserOutput::Success(
        SystemFieldUpdates {
            ips: ips.into_iter().map(|ip| ip.to_string()).collect(),
            ..Default::default()
        },
        detail,
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

fn parse_ip_addrs(stdout: &str) -> Vec<IpAddr> {
    let mut ips = Vec::new();
    for token in stdout.split_whitespace() {
        match token.parse::<IpAddr>() {
            Ok(ip) if !ips.contains(&ip) => ips.push(ip),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    ips
}

fn parse_env_vars(stdout: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    let sep = if stdout.contains('\0') { '\0' } else { '\n' };

    for raw_line in stdout.split(sep) {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        let Some((k, v)) = line.split_once('=') else {
            continue;
        };

        if k.trim().is_empty() {
            continue;
        }

        vars.insert(k.to_string(), v.to_string());
    }

    vars
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
    use ran_domain::{Entity, Pod};

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
    fn parses_standard_env_output_fixture() {
        let stdout_fixture = "HOME=/root\nPATH=/usr/local/sbin:/usr/local/bin\nKUBERNETES_SERVICE_HOST=10.96.0.1\n";
        let parsed = parse_env_vars(stdout_fixture);

        assert_eq!(parsed.get("HOME"), Some(&"/root".to_string()));
        assert_eq!(
            parsed.get("KUBERNETES_SERVICE_HOST"),
            Some(&"10.96.0.1".to_string())
        );
        assert_eq!(parsed.len(), 3);
    }

    #[test]
    fn parses_null_delimited_env_output_fixture() {
        let stdout_fixture = "HOME=/root\0PATH=/bin\0";
        let parsed = parse_env_vars(stdout_fixture);

        assert_eq!(parsed.get("HOME"), Some(&"/root".to_string()));
        assert_eq!(parsed.get("PATH"), Some(&"/bin".to_string()));
        assert_eq!(parsed.len(), 2);
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
        campaign.pods.insert(pod.entity_id(), pod);

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
        campaign.pods.insert(pod.entity_id(), pod);

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
        campaign.pods.insert(pod.entity_id(), pod);

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

    // --- sys.ip tests ---

    #[test]
    fn parse_ip_addrs_parses_mixed_ipv4_and_ipv6() {
        let ips = parse_ip_addrs("10.0.0.1 192.168.1.5 ::1");
        assert_eq!(ips.len(), 3);
        assert!(ips.iter().any(|ip| ip.to_string() == "10.0.0.1"));
        assert!(ips.iter().any(|ip| ip.to_string() == "192.168.1.5"));
        assert!(ips.iter().any(|ip| ip.to_string() == "::1"));
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
        campaign.pods.insert(pod.entity_id(), pod);

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
}
