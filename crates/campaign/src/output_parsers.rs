use std::collections::HashMap;

use c2::{ExecTtp, TtpExecuted};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

type OutputParserHandler = fn(
    campaign: &mut Campaign,
    effect_id: &str,
    cmd: &ExecTtp,
    event: &TtpExecuted,
) -> ParsedEffect;

pub fn parse_output_effect(
    campaign: &mut Campaign,
    effect_id: &str,
    cmd: &ExecTtp,
    event: &TtpExecuted,
) -> Option<ParsedEffect> {
    resolve_output_parser(effect_id).map(|handler| handler(campaign, effect_id, cmd, event))
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

fn resolve_output_parser(effect_name: &str) -> Option<OutputParserHandler> {
    match effect_name.trim().to_ascii_lowercase().as_str() {
        "sys.envvar" => Some(parse_sys_envvar),
        _ => None,
    }
}

fn parse_sys_envvar(
    campaign: &mut Campaign,
    effect_id: &str,
    cmd: &ExecTtp,
    event: &TtpExecuted,
) -> ParsedEffect {
    let stderr = event.results.get(1).cloned().unwrap_or_default();

    let Some(stdout) = event.results.first() else {
        return ParsedEffect {
            updates: FactsUpdate::default(),
            audit: build_audit(
                effect_id,
                cmd,
                event,
                ParseResult::KnownFailure,
                "missing stdout payload",
                0,
            ),
        };
    };

    let vars = parse_env_vars(stdout);
    if vars.is_empty() && !stdout.trim().is_empty() {
        return ParsedEffect {
            updates: FactsUpdate::default(),
            audit: build_audit(
                effect_id,
                cmd,
                event,
                ParseResult::UnknownFormat,
                "stdout did not contain parseable KEY=VALUE lines",
                0,
            ),
        };
    }

    let Some(mut target) = get_parse_target_system(campaign, cmd) else {
        return ParsedEffect {
            updates: FactsUpdate::default(),
            audit: build_audit(
                effect_id,
                cmd,
                event,
                ParseResult::KnownFailure,
                "target is not a system entity (checked target_id and exec_system_id)",
                0,
            ),
        };
    };

    let sys = target.entity_mut().system_mut();
    let before = sys.env_vars.len();
    sys.env_vars.extend(vars);
    let after = sys.env_vars.len();

    ParsedEffect {
        updates: FactsUpdate::default(),
        audit: build_audit(
            effect_id,
            cmd,
            event,
            ParseResult::Parsed,
            if stderr.trim().is_empty() {
                "parsed and merged environment variables"
            } else {
                "parsed and merged environment variables (stderr had non-fatal content)"
            },
            after.saturating_sub(before),
        ),
    }
}

fn get_parse_target_system<'a>(
    campaign: &'a mut Campaign,
    cmd: &ExecTtp,
) -> Option<crate::CampaignSystemEntityMut<'a>> {
    let target_id = if campaign.get_system_entity(&cmd.target_id).is_some() {
        Some(cmd.target_id.as_str())
    } else if !cmd.exec_system_id.trim().is_empty()
        && campaign.get_system_entity(&cmd.exec_system_id).is_some()
    {
        Some(cmd.exec_system_id.as_str())
    } else {
        None
    }?;

    campaign.get_system_entity_mut(target_id)
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
}
