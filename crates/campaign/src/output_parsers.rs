use std::collections::HashMap;
use std::net::IpAddr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use c2::{ExecTtp, TtpExecuted};
use ran_domain::{
    AccessLevel, Contains, Entity, EntityId, JwToken, K8sNode, Mount, Namespace, Pod, Process,
    RbacPermission, RunsOn, ServiceAccount, ServiceAccountToken, Uses,
};
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
#[derive(Debug)]
enum ParserOutput {
    /// Parser updated system-level fields on the target entity.
    Success(SystemFieldUpdates, String),
    /// Parser produced new entities and relations (not tied to a system target).
    SuccessWithFacts(FactsUpdate, String),
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
    // Resolve the parser. Parametrized effects (e.g. `sys.has-binary(...)`) carry
    // their argument inside the effect-ID string; extract it and call the pure fn
    // directly, then fall through to the shared merge path below.
    let normalized = effect_id.trim().to_ascii_lowercase();

    let stdout = match event.results.first() {
        Some(s) => s.as_str(),
        None => {
            // Only return early if there is actually a registered/known parser.
            let is_known = normalized.starts_with("sys.has-binary(")
                || normalized == "k8s.selfsubjectrulesreview"
                || resolve_output_parser(&normalized).is_some();
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
    };
    let stderr = event.results.get(1).map(String::as_str).unwrap_or("");

    let parser_output: ParserOutput = if normalized.starts_with("sys.has-binary(") {
        let inner = extract_effect_args(effect_id).unwrap_or("");
        parse_sys_has_binary(stdout, inner)
    } else if normalized == "k8s.selfsubjectrulesreview" {
        parse_self_subject_rules_review(stdout, stderr, &cmd.target_id, campaign)
    } else {
        let parser = resolve_output_parser(&normalized)?;
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

fn resolve_output_parser(effect_name: &str) -> Option<InternalParserFn> {
    match effect_name.trim().to_ascii_lowercase().as_str() {
        "sys.envvar" => Some(parse_sys_envvar),
        "sys.ip" => Some(parse_sys_ip),
        "sys.processes" => Some(parse_sys_processes),
        "sys.userid" => Some(parse_sys_userid),
        "linux.mounts" => Some(parse_linux_mounts),
        "rawserviceaccounttoken" => Some(parse_raw_service_account_token),
        "rdns" => Some(parse_rdns),
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

fn parse_sys_processes(stdout: &str, _stderr: &str) -> ParserOutput {
    let lines: Vec<&str> = stdout.split('\n').collect();
    if lines.len() < 2 {
        return ParserOutput::KnownFailure("no process entries found in output".to_string());
    }

    let mut procs = Vec::new();
    for line in lines.iter().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        match parse_process_line(line) {
            Some(p) => procs.push(p),
            None => {
                return ParserOutput::UnknownFormat(format!(
                    "failed to parse process line: {}",
                    line
                ))
            }
        }
    }

    let detail = format!("parsed {} process(es)", procs.len());
    ParserOutput::Success(
        SystemFieldUpdates {
            processes: procs,
            ..Default::default()
        },
        detail,
    )
}

/// Parse a single `ps`-style line.
///
/// Expected format (at least 8 whitespace-separated fields):
/// ```text
/// USER  PID  PPID  CPU  STARTTIME  TTY  TIME  CMD...
/// ```
fn parse_process_line(line: &str) -> Option<Process> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 8 {
        return None;
    }

    let pid: u32 = fields[1].parse().ok()?;
    let parent_pid: u32 = fields[2].parse().ok()?;
    let cmd = fields[7..].join(" ");
    let name = fields[7]
        .split('/')
        .next_back()
        .unwrap_or(fields[7])
        .to_string();

    Some(Process {
        pid,
        parent_pid,
        name,
        cmd,
        user: Some(fields[0].to_string()),
        start_time: Some(fields[4].to_string()),
    })
}

/// Parse `id` command output: `uid=0(root) gid=0(root) groups=0(root),1(bin)`.
///
/// Extracts the numeric uid and the username in parentheses.  Sets
/// `access_level` to `RootExec` for uid 0, `UserExec` for any other uid.
fn parse_sys_userid(stdout: &str, _stderr: &str) -> ParserOutput {
    let line = stdout.trim();
    if line.is_empty() {
        return ParserOutput::KnownFailure("empty output from id command".to_string());
    }

    // uid=<number>(<name>)
    let Some(uid_part) = line.split_whitespace().next() else {
        return ParserOutput::UnknownFormat(format!("unexpected id output format: {line}"));
    };

    let uid_part = uid_part.strip_prefix("uid=").unwrap_or(uid_part);

    let (uid_str, username) = if let Some((num, rest)) = uid_part.split_once('(') {
        let name = rest.trim_end_matches(')');
        (num, Some(name.to_string()))
    } else {
        (uid_part, None)
    };

    let Ok(uid) = uid_str.parse::<u32>() else {
        return ParserOutput::UnknownFormat(format!("could not parse uid from: {line}"));
    };

    let access_level = if uid == 0 {
        AccessLevel::RootExec
    } else {
        AccessLevel::UserExec
    };

    let detail = match &username {
        Some(name) => format!("uid={uid} ({name}), access_level={access_level:?}"),
        None => format!("uid={uid}, access_level={access_level:?}"),
    };

    ParserOutput::Success(
        SystemFieldUpdates {
            user_id: Some(uid),
            username,
            access_level: Some(access_level),
            ..Default::default()
        },
        detail,
    )
}

/// Parse mount table output into `Mount` entries.
///
/// Supports two formats:
///
/// **`/proc/self/mountinfo`** (kernel format):
/// ```text
/// 22 28 0:21 / /sys rw,nosuid,nodev shared:7 - sysfs sysfs rw
/// ```
/// Fields: mountid parentid major:minor root mountpoint opts optional... `-` fstype source subopts
///
/// **`mount` command** (human format):
/// ```text
/// sysfs on /sys type sysfs (rw,nosuid,nodev)
/// ```
/// Pattern: `<source> on <mountpoint> type <fstype> (<options>)`
fn parse_linux_mounts(stdout: &str, _stderr: &str) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty mount output".to_string());
    }

    let mut mounts = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(m) = parse_mountinfo_line(line).or_else(|| parse_mount_cmd_line(line)) {
            mounts.push(m);
        }
        // Unrecognised lines are silently skipped — mixed output can happen.
    }

    if mounts.is_empty() {
        return ParserOutput::UnknownFormat(
            "no mount entries recognised in output".to_string(),
        );
    }

    let detail = format!("parsed {} mount(s)", mounts.len());
    ParserOutput::Success(
        SystemFieldUpdates {
            mounts,
            ..Default::default()
        },
        detail,
    )
}

/// Parse a single `/proc/self/mountinfo` line.
///
/// Format: `mountid parentid major:minor root mountpoint mountopts [optfields] - fstype source subopts`
fn parse_mountinfo_line(line: &str) -> Option<Mount> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    // Minimum: id parent major:minor root mountpoint opts - fstype source
    if fields.len() < 9 {
        return None;
    }

    // Fields 0..1 are numeric ids, field 2 is major:minor
    fields[0].parse::<u32>().ok()?;
    fields[1].parse::<u32>().ok()?;
    if !fields[2].contains(':') {
        return None;
    }

    let mount_root = fields[3].to_string();
    let mount_point = fields[4].to_string();

    // Find the `-` separator for the filesystem type section
    let dash_pos = fields.iter().position(|&f| f == "-")?;
    let fs_type = fields.get(dash_pos + 1).unwrap_or(&"").to_string();

    let is_host_path = is_kubelet_host_path(&mount_point);

    Some(Mount {
        name: String::new(),
        mount_point,
        mount_root,
        mount_type: if fs_type.is_empty() { None } else { Some(fs_type) },
        read_only: fields[5].contains("ro"),
        is_host_path,
    })
}

/// Parse a single `mount` command output line.
///
/// Format: `<source> on <mountpoint> type <fstype> (<options>)`
fn parse_mount_cmd_line(line: &str) -> Option<Mount> {
    // Must contain " on " and " type "
    let on_pos = line.find(" on ")?;
    let after_on = &line[on_pos + 4..];
    let type_pos = after_on.find(" type ")?;

    let mount_point = after_on[..type_pos].trim().to_string();
    let after_type = &after_on[type_pos + 6..];

    let fs_type = after_type
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();

    let read_only = after_type.contains("(ro") || after_type.contains(",ro");
    let is_host_path = is_kubelet_host_path(&mount_point);

    Some(Mount {
        name: String::new(),
        mount_point,
        mount_root: String::new(),
        mount_type: if fs_type.is_empty() { None } else { Some(fs_type) },
        read_only,
        is_host_path,
    })
}

/// Returns `true` when the mount point is a kubelet-managed host path,
/// indicating the pod has visibility into the node's filesystem.
fn is_kubelet_host_path(mount_point: &str) -> bool {
    mount_point.contains("/var/lib/kubelet")
}

// ---------------------------------------------------------------------------
// rawServiceAccountToken
// ---------------------------------------------------------------------------

/// Internal structs for deserializing the Kubernetes JWT payload.
#[derive(Debug, Deserialize)]
struct JwtPayload {
    sub: Option<String>,
    #[serde(default)]
    aud: serde_json::Value,
    iss: Option<String>,
    exp: Option<i64>,
    iat: Option<i64>,
    #[serde(rename = "kubernetes.io")]
    kubernetes: Option<KubernetesPayload>,
    // Legacy (non-projected) SA token fields.
    #[serde(rename = "kubernetes.io/serviceaccount/namespace")]
    legacy_namespace: Option<String>,
    #[serde(rename = "kubernetes.io/serviceaccount/service-account.name")]
    legacy_sa_name: Option<String>,
    #[serde(rename = "kubernetes.io/serviceaccount/service-account.uid")]
    legacy_sa_uid: Option<String>,
    #[serde(rename = "kubernetes.io/serviceaccount/pod.name")]
    legacy_pod_name: Option<String>,
    #[serde(rename = "kubernetes.io/serviceaccount/pod.uid")]
    legacy_pod_uid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KubernetesPayload {
    namespace: Option<String>,
    pod: Option<ResourceRef>,
    node: Option<ResourceRef>,
    serviceaccount: Option<ResourceRef>,
}

#[derive(Debug, Deserialize)]
struct ResourceRef {
    name: Option<String>,
    uid: Option<String>,
}

/// Parse a raw Kubernetes ServiceAccount JWT from stdout and produce new
/// entities (ServiceAccount, Namespace, Pod, Node) and relations.
///
/// Mirrors Go's `parseRawServiceAccountToken` + `analyzeServiceAccountToken`.
///
/// Handles multi-line stdout: searches for the first line containing `ey`
/// and `.`, which is the hallmark of a base64url-encoded JWT.
fn parse_raw_service_account_token(stdout: &str, _stderr: &str) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty output — no token provided".to_string());
    }

    // Find the JWT within possibly multi-line output.
    let token_str = find_jwt_in_output(stdout);
    if token_str.is_empty() {
        return ParserOutput::KnownFailure(
            "could not locate a JWT token in output".to_string(),
        );
    }

    // Decode the JWT payload (second of three dot-separated segments).
    let parts: Vec<&str> = token_str.splitn(3, '.').collect();
    if parts.len() != 3 {
        return ParserOutput::UnknownFormat(format!(
            "expected 3 JWT segments, got {}",
            parts.len()
        ));
    }

    let payload_bytes = match URL_SAFE_NO_PAD.decode(parts[1]) {
        Ok(b) => b,
        Err(e) => {
            return ParserOutput::UnknownFormat(format!(
                "failed to base64-decode JWT payload: {e}"
            ))
        }
    };

    let payload: JwtPayload = match serde_json::from_slice(&payload_bytes) {
        Ok(p) => p,
        Err(e) => {
            return ParserOutput::UnknownFormat(format!(
                "failed to parse JWT payload JSON: {e}"
            ))
        }
    };

    // Resolve namespace and SA name from either projected or legacy claims.
    let (namespace, sa_name, sa_uid, pod_name, pod_uid, node_name) =
        resolve_k8s_claims(&payload);

    if namespace.is_empty() || sa_name.is_empty() {
        return ParserOutput::UnknownFormat(
            "JWT payload missing required kubernetes namespace or serviceaccount claims"
                .to_string(),
        );
    }

    // Build the audience list for JwToken.
    let audience = match &payload.aud {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        serde_json::Value::String(s) => vec![s.clone()],
        _ => vec![],
    };

    let jwt = JwToken {
        raw: token_str.to_string(),
        subject: payload.sub.clone(),
        audience,
        issuer: payload.iss.clone(),
        expires_at: payload.exp,
        issued_at: payload.iat,
    };

    let is_bound = pod_uid.as_deref().map(|u| !u.is_empty()).unwrap_or(false);

    let token = ServiceAccountToken {
        jwt,
        namespace: namespace.clone(),
        service_account_name: sa_name.clone(),
        service_account_uid: sa_uid,
        pod_name: pod_name.clone(),
        pod_uid,
        is_bound,
    };

    // Assemble FactsUpdate.
    let mut facts = FactsUpdate::default();

    // Namespace.
    let ns = Namespace::new(&namespace);
    let ns_id = ns.entity_id();
    facts.new_entities.push(Box::new(ns));

    // ServiceAccount (with token).
    let mut sa = ServiceAccount::new(&sa_name, &namespace);
    sa.token = Some(token);
    let sa_id = sa.entity_id();
    facts.new_entities.push(Box::new(sa));

    // Contains: namespace → SA.
    facts
        .new_relations
        .push(Box::new(Contains::new(ns_id.0.clone(), sa_id.0.clone())));

    // Pod (if the token carries pod claims — always true for bound tokens and
    // most legacy tokens that include pod info).
    if let Some(pod_name) = &pod_name {
        if !pod_name.is_empty() {
            let mut pod = Pod::new(pod_name.as_str(), namespace.as_str());
            pod.service_account_name = Some(sa_name.clone());
            pod.is_running = true;
            let pod_id = pod.entity_id();

            // If bound, attach the node name.
            if let Some(node_name) = &node_name {
                if !node_name.is_empty() {
                    pod.node_name = Some(node_name.clone());

                    let node = K8sNode::new(node_name.as_str());
                    let node_id = node.entity_id();
                    facts.new_entities.push(Box::new(node));
                    facts.new_relations.push(Box::new(RunsOn::new(
                        pod_id.0.clone(),
                        node_id.0.clone(),
                    )));
                }
            }

            facts.new_entities.push(Box::new(pod));

            // Uses: pod → SA.
            facts
                .new_relations
                .push(Box::new(Uses::new(pod_id.0.clone(), sa_id.0.clone())));
        }
    }

    let entity_count = facts.new_entities.len();
    let relation_count = facts.new_relations.len();
    let detail = format!(
        "decoded SA token for {}/{}: {} entities, {} relations",
        namespace, sa_name, entity_count, relation_count
    );

    ParserOutput::SuccessWithFacts(facts, detail)
}

/// Extract the JWT string from possibly multi-line output.
///
/// A JWT starts with a base64url-encoded header, so the first segment always
/// starts with `ey` (base64 of `{"`).  The token must also contain at least
/// two `.` separators.
fn find_jwt_in_output(stdout: &str) -> &str {
    // Single-line (common case): the whole trimmed output is the token.
    let trimmed = stdout.trim();
    if !trimmed.contains('\n') {
        return trimmed;
    }

    // Multi-line: search for the JWT line.
    for line in stdout.lines() {
        let line = line.trim();
        if line.contains("ey") && line.contains('.') {
            return line;
        }
    }

    ""
}

/// Resolve Kubernetes claims from a JWT payload, supporting both projected
/// (new-style `kubernetes.io` claim) and legacy flat claim formats.
///
/// Returns `(namespace, sa_name, sa_uid, pod_name, pod_uid, node_name)`.
fn resolve_k8s_claims(
    payload: &JwtPayload,
) -> (String, String, Option<String>, Option<String>, Option<String>, Option<String>) {
    if let Some(k8s) = &payload.kubernetes {
        let namespace = k8s.namespace.clone().unwrap_or_default();
        let sa_name = k8s
            .serviceaccount
            .as_ref()
            .and_then(|sa| sa.name.clone())
            .unwrap_or_default();
        let sa_uid = k8s
            .serviceaccount
            .as_ref()
            .and_then(|sa| sa.uid.clone());
        let pod_name = k8s.pod.as_ref().and_then(|p| p.name.clone());
        let pod_uid = k8s.pod.as_ref().and_then(|p| p.uid.clone());
        let node_name = k8s.node.as_ref().and_then(|n| n.name.clone());
        (namespace, sa_name, sa_uid, pod_name, pod_uid, node_name)
    } else {
        // Legacy flat claims.
        let namespace = payload.legacy_namespace.clone().unwrap_or_default();
        let sa_name = payload.legacy_sa_name.clone().unwrap_or_default();
        let sa_uid = payload.legacy_sa_uid.clone();
        let pod_name = payload.legacy_pod_name.clone();
        let pod_uid = payload.legacy_pod_uid.clone();
        (namespace, sa_name, sa_uid, pod_name, pod_uid, None)
    }
}

// ---------------------------------------------------------------------------
// rdns
// ---------------------------------------------------------------------------

/// Parser for the `rdns` effect — reverse DNS lookup results.
///
/// Expected stdout format (CSV with optional header):
/// ```text
/// ip,ptr
/// 10.244.1.4,10-244-1-4.backend-service.dev.svc.cluster.local
/// 10.244.1.6,10-244-1-6.argocd-server.argocd.svc.cluster.local
/// 192.168.0.5,host.local
/// ```
///
/// Only `cluster.local` entries are processed. Entries whose first DNS label
/// matches the IP in kebab-case form (e.g. `10-244-1-4`) are inferred to be
/// Pod addresses; other entries (service VIPs, external hosts) are skipped
/// because there is no Service domain type yet.
fn parse_rdns(stdout: &str, _stderr: &str) -> ParserOutput {
    let data = stdout.trim();
    if data.is_empty() {
        return ParserOutput::KnownFailure("empty rDNS output".to_string());
    }

    // Parse "ip,ptr" lines, skipping the optional header and malformed lines.
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() || line.eq_ignore_ascii_case("ip,ptr") {
            continue;
        }
        let mut cols = line.splitn(2, ',');
        let Some(ip_str) = cols.next() else { continue };
        let Some(dns) = cols.next() else { continue };
        let ip_str = ip_str.trim();
        let dns = dns.trim();
        if ip_str.parse::<IpAddr>().is_err() {
            continue;
        }
        entries.push((ip_str.to_string(), dns.to_string()));
    }

    if entries.is_empty() {
        return ParserOutput::KnownFailure("no valid IP,DNS entries found in rDNS output".to_string());
    }

    let mut facts = FactsUpdate::default();
    let mut pod_count = 0usize;

    for (ip_str, dns_str) in &entries {
        // Only process Kubernetes cluster-local entries.
        if !dns_str.ends_with("cluster.local") {
            continue;
        }

        let Ok(ip) = ip_str.parse::<IpAddr>() else {
            continue;
        };

        let dns_parts: Vec<&str> = dns_str.split('.').collect();
        let ip_kebab = ip_str.replace('.', "-");

        // Derive pod name and namespace from the DNS label structure.
        // Mirrors Go's analyzeDnsEntries label-count logic.
        let (name, ns) = match dns_parts.len() {
            4 => (dns_parts[0].to_string(), dns_parts[0].to_string()),
            5 => (dns_parts[0].to_string(), dns_parts[1].to_string()),
            6 => (
                format!("{}.{}", dns_parts[1], dns_parts[0]),
                dns_parts[2].to_string(),
            ),
            n if n > 6 => (dns_parts[0].to_string(), dns_parts[2].to_string()),
            _ => continue,
        };

        // Skip service VIPs — only pod entries have the IP as the first label.
        let is_pod = dns_parts.first().map_or(false, |&l| l == ip_kebab);
        if !is_pod {
            continue;
        }

        let mut pod = Pod::new(&name, &ns);
        pod.system.ips.push(ip);
        facts.new_entities.push(Box::new(pod));
        pod_count += 1;
    }

    if pod_count == 0 {
        return ParserOutput::KnownFailure(
            "no pod entries found in rDNS output (entries may be service VIPs or non-cluster hosts)".to_string(),
        );
    }

    ParserOutput::SuccessWithFacts(
        facts,
        format!("discovered {} pod(s) from rDNS", pod_count),
    )
}

// ---------------------------------------------------------------------------
// sys.has-binary
// ---------------------------------------------------------------------------

/// Pure parser for `sys.has-binary(...)` effects.
///
/// `inner` is the already-extracted text between the parentheses, e.g.
/// `/usr/bin/nmap`, `'ran-ws', /tmp/ran-ws`, or `${OUTPUT}`.
/// The effect-ID string has already been through `ground_template` so template
/// variables like `${BIN_PATH}` are already substituted by the time we see it.
fn parse_sys_has_binary(stdout: &str, inner: &str) -> ParserOutput {
    let (explicit_name, source) = split_has_binary_args(inner);
    let is_output = source.eq_ignore_ascii_case("${output}")
        || source.eq_ignore_ascii_case("output");

    let binaries: HashMap<String, String> = if is_output {
        let paths = parse_binary_paths_from_output(stdout);
        if paths.is_empty() {
            return ParserOutput::KnownFailure("no binary paths found in stdout".to_string());
        }
        paths
            .into_iter()
            .map(|path| {
                let name = explicit_name.clone().unwrap_or_else(|| {
                    path.rsplit('/').next().unwrap_or(&path).to_string()
                });
                (name, path)
            })
            .collect()
    } else {
        let bin_path = source.to_string();
        if bin_path.is_empty() {
            return ParserOutput::KnownFailure(
                "sys.has-binary effect had empty argument".to_string(),
            );
        }
        let name = explicit_name.unwrap_or_else(|| {
            if bin_path.contains('/') {
                bin_path.rsplit('/').next().unwrap_or(&bin_path).to_string()
            } else {
                bin_path.clone()
            }
        });
        let mut m = HashMap::new();
        m.insert(name, bin_path);
        m
    };

    let detail = format!("recorded {} binary/binaries", binaries.len());
    ParserOutput::Success(
        SystemFieldUpdates {
            binaries,
            ..Default::default()
        },
        detail,
    )
}

/// Extract the text between the outermost `(` and `)` of an effect string.
fn extract_effect_args(effect: &str) -> Option<&str> {
    let open = effect.find('(')?;
    let close = effect.rfind(')')?;
    if close <= open {
        return None;
    }
    Some(effect[open + 1..close].trim())
}

/// Split `has-binary` inner args into `(explicit_name, source)`.
///
/// - Single-arg `"/usr/bin/nmap"` → `(None, "/usr/bin/nmap")`
/// - Two-arg `"ran-ws, /tmp/ran-ws"` → `(Some("ran-ws"), "/tmp/ran-ws")`
/// - Quoted name `"'ran-ws', ${OUTPUT}"` → `(Some("ran-ws"), "${OUTPUT}")`
fn split_has_binary_args(inner: &str) -> (Option<String>, &str) {
    if let Some(comma_pos) = inner.find(',') {
        let name_part = inner[..comma_pos].trim().trim_matches(|c| c == '\'' || c == '"');
        let rest = inner[comma_pos + 1..].trim();
        // Empty first arg (`, /path` form) → derive name from path
        if name_part.is_empty() {
            (None, rest)
        } else {
            (Some(name_part.to_string()), rest)
        }
    } else {
        (None, inner)
    }
}

/// Extract absolute binary paths from stdout.
///
/// Rules (mirrors Go `parseBinaryPathsFromOutput`):
/// - Must start with `/`
/// - No spaces
/// - No `...` (apt/dpkg progress lines)
/// - At least two `/` characters (path depth ≥ 2)
fn parse_binary_paths_from_output(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| line.starts_with('/'))
        .filter(|line| !line.contains(' '))
        .filter(|line| !line.contains("..."))
        .filter(|line| line.chars().filter(|&c| c == '/').count() >= 2)
        .map(String::from)
        .collect()
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

// ---------------------------------------------------------------------------
// k8s.SelfSubjectRulesReview
// ---------------------------------------------------------------------------

/// Deserializable form of the Kubernetes `SelfSubjectRulesReview` API response.
#[derive(Debug, Deserialize)]
struct SsrrResponse {
    status: Option<SsrrStatus>,
    code: Option<u32>,
    message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SsrrStatus {
    #[serde(rename = "resourceRules", default)]
    resource_rules: Vec<SsrrResourceRule>,
    #[serde(rename = "nonResourceRules", default)]
    non_resource_rules: Vec<SsrrNonResourceRule>,
    #[serde(default)]
    incomplete: bool,
}

#[derive(Debug, Deserialize)]
struct SsrrResourceRule {
    #[serde(default)]
    verbs: Vec<String>,
    #[serde(rename = "apiGroups", default)]
    api_groups: Vec<String>,
    #[serde(default)]
    resources: Vec<String>,
    #[serde(rename = "resourceNames", default)]
    resource_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SsrrNonResourceRule {
    #[serde(default)]
    verbs: Vec<String>,
    #[serde(rename = "nonResourceURLs", default)]
    non_resource_urls: Vec<String>,
}

/// Parse a `k8s.SelfSubjectRulesReview` effect output into RBAC entitlements
/// on the target ServiceAccount.
///
/// Supports two output formats:
/// - JSON: the raw Kubernetes API response from `curl … /selfsubjectrulesreviews`
/// - Pretty: the tabular output of `kubectl auth can-i --list`
///
/// The resulting `RbacPermission` entries are attached to the SA. The existing SA
/// is cloned from the campaign so that the token and other fields are preserved.
fn parse_self_subject_rules_review(
    stdout: &str,
    _stderr: &str,
    target_id: &str,
    campaign: &Campaign,
) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty output".to_string());
    }

    // Determine format and extract rules.
    let rules = if serde_json::from_str::<serde_json::Value>(stdout.trim()).is_ok() {
        let resp: SsrrResponse = match serde_json::from_str(stdout.trim()) {
            Ok(r) => r,
            Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
        };
        if resp.code.map(|c| c >= 400).unwrap_or(false) {
            return ParserOutput::KnownFailure(format!(
                "SelfSubjectRulesReview API error (code {}): {}",
                resp.code.unwrap_or(0),
                resp.message.unwrap_or_default()
            ));
        }
        let status = resp.status.unwrap_or_default();
        if status.incomplete {
            tracing::warn!("SelfSubjectRulesReview results are incomplete");
        }
        (status.resource_rules, status.non_resource_rules)
    } else {
        match parse_kubectl_ssrr_table(stdout) {
            Ok(rules) => rules,
            Err(e) => return ParserOutput::UnknownFormat(format!("pretty-print parse error: {e}")),
        }
    };
    let (resource_rules, non_resource_rules) = rules;

    // Resolve the ServiceAccount to update.
    // target_id is the *exec target* (often a pod that ran the command), not
    // necessarily the SA being reviewed. Follow pod → SA if needed.
    let Some(mut sa) = resolve_sa_for_ssrr(target_id, campaign) else {
        return ParserOutput::KnownFailure(format!(
            "cannot resolve a ServiceAccount for target '{target_id}': \
             target is neither a known SA nor a pod with a known service account"
        ));
    };

    let sa_namespace = sa.meta.namespace.as_deref().unwrap_or("").to_string();
    let mut entitlements: Vec<RbacPermission> = Vec::new();

    for rule in &resource_rules {
        for verb in &rule.verbs {
            for resource in &rule.resources {
                let api_groups: &[String] = if rule.api_groups.is_empty() {
                    &[]
                } else {
                    &rule.api_groups
                };

                // Treat empty api_groups slice as a single entry with the core group ("").
                let effective_groups: Vec<&str> = if api_groups.is_empty() {
                    vec![""]
                } else {
                    api_groups.iter().map(String::as_str).collect()
                };

                for api_group in &effective_groups {
                    let scope = if is_namespaced_resource(resource, api_group) && !sa_namespace.is_empty() {
                        Some(sa_namespace.clone())
                    } else {
                        None
                    };

                    if rule.resource_names.is_empty() {
                        let mut perm = RbacPermission::new(verb, resource);
                        perm.api_group = Some(api_group.to_string());
                        perm.scope = scope;
                        entitlements.push(perm);
                    } else {
                        for resource_name in &rule.resource_names {
                            let mut perm = RbacPermission::new(verb, resource);
                            perm.api_group = Some(api_group.to_string());
                            perm.resource_name = Some(resource_name.clone());
                            perm.scope = scope.clone();
                            entitlements.push(perm);
                        }
                    }
                }
            }
        }
    }

    for rule in &non_resource_rules {
        for verb in &rule.verbs {
            for url in &rule.non_resource_urls {
                let mut perm = RbacPermission::new(verb, "");
                perm.resource_name = Some(url.clone());
                entitlements.push(perm);
            }
        }
    }

    let perm_count = entitlements.len();
    sa.entitlements = entitlements;

    let mut facts = FactsUpdate::default();
    facts.new_entities.push(Box::new(sa));

    ParserOutput::SuccessWithFacts(
        facts,
        format!("parsed {} RBAC permission(s) from SelfSubjectRulesReview", perm_count),
    )
}

/// Parse the tabular output of `kubectl auth can-i --list` into resource and
/// non-resource rule lists.
///
/// The format uses `[...]` delimiters for three of the four columns:
/// ```text
/// Resources   Non-Resource URLs   Resource Names   Verbs
/// pods        []                  []               [get list]
///             [/api]              []               [get]
/// ```
fn parse_kubectl_ssrr_table(
    data: &str,
) -> Result<(Vec<SsrrResourceRule>, Vec<SsrrNonResourceRule>), String> {
    let mut resource_rules = Vec::new();
    let mut non_resource_rules = Vec::new();

    let mut lines = data.lines();
    // Skip the header row.
    let _ = lines.next();

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }

        // The three bracketed columns are delimited by `[`.  Split into at most 4
        // parts: [resources_col, urls_col, names_col, verbs_col].
        let parts: Vec<&str> = line.splitn(4, '[').collect();
        if parts.len() < 4 {
            continue;
        }

        fn strip_bracket_and_split(s: &str) -> Vec<String> {
            let cleaned = s.trim().trim_end_matches(']');
            if cleaned.is_empty() {
                vec![]
            } else {
                cleaned.split_whitespace().map(String::from).collect()
            }
        }

        let resources_raw = parts[0].trim();
        let non_resource_urls = strip_bracket_and_split(parts[1]);
        let resource_names = strip_bracket_and_split(parts[2]);
        let verbs = strip_bracket_and_split(parts[3]);

        if verbs.is_empty() {
            continue;
        }

        if resources_raw.is_empty() {
            // Non-resource rule — the resources column is blank.
            non_resource_rules.push(SsrrNonResourceRule { verbs, non_resource_urls });
        } else {
            // Resource rule — split `resource.apiGroup` from the resources column.
            let (resource, api_group) = split_resource_api_group(resources_raw);
            resource_rules.push(SsrrResourceRule {
                verbs,
                api_groups: vec![api_group],
                resources: vec![resource],
                resource_names,
            });
        }
    }

    Ok((resource_rules, non_resource_rules))
}

/// Split a `resource[.apiGroup]` string from kubectl pretty output into
/// `(resource, apiGroup)`.
///
/// | Input | resource | apiGroup |
/// |-------|----------|----------|
/// | `*.*` | `*` | `*` |
/// | `pods` | `pods` | `""` |
/// | `pods/exec` | `pods/exec` | `""` |
/// | `selfsubjectrulesreviews.authorization.k8s.io` | `selfsubjectrulesreviews` | `authorization.k8s.io` |
fn split_resource_api_group(s: &str) -> (String, String) {
    if s == "*.*" {
        return ("*".to_string(), "*".to_string());
    }
    if let Some(dot) = s.find('.') {
        // Only treat `.` as an apiGroup separator when the resource part has no
        // `/` (subresource), e.g. `pods/exec` should not be split.
        let resource_part = &s[..dot];
        if !resource_part.contains('/') {
            return (resource_part.to_string(), s[dot + 1..].to_string());
        }
    }
    (s.to_string(), String::new())
}

/// Resolve the `ServiceAccount` that a SSRR result should be attached to.
///
/// `target_id` is the exec target — typically the pod that ran the command,
/// not the SA itself (the SA's exec channel resolves via its pod).
///
/// Resolution order:
/// 1. `target_id` is already a known SA → return it directly.
/// 2. `target_id` is a known pod → follow `pod.service_account_name` to the SA.
///    If that SA doesn't exist in the campaign yet, create a minimal stub.
/// 3. Returns `None` when neither condition is met.
fn resolve_sa_for_ssrr(target_id: &str, campaign: &Campaign) -> Option<ServiceAccount> {
    let id = EntityId::new(target_id);

    // Case 1: target already is a ServiceAccount.
    if let Some(sa) = campaign.service_accounts.get(&id) {
        return Some(sa.clone());
    }

    // Case 2: target is a pod — look up the SA the pod uses.
    if let Some(pod) = campaign.pods.get(&id) {
        let sa_name = pod.service_account_name.as_deref()?;
        let namespace = pod.namespace()?;
        let sa_id = EntityId::new(format!("ns/{}/sa/{}", namespace, sa_name));
        let sa = campaign
            .service_accounts
            .get(&sa_id)
            .cloned()
            .unwrap_or_else(|| ServiceAccount::new(sa_name, namespace));
        return Some(sa);
    }

    None
}

/// Returns `true` when `resource` in `api_group` is namespaced.
///
/// Unknown resources default to `true` (namespaced).  Wildcards (`"*"`) span
/// both scopes — treated as cluster-scoped (`false`) to avoid over-constraining
/// the permission scope.
fn is_namespaced_resource(resource: &str, api_group: &str) -> bool {
    if resource == "*" || api_group == "*" {
        return false;
    }

    let name = resource.to_ascii_lowercase();
    let group = api_group.to_ascii_lowercase();

    let cluster_scoped: &[(&str, &[&str])] = &[
        ("", &[
            "componentstatuses", "componentstatus",
            "namespaces", "namespace",
            "nodes", "node",
            "persistentvolumes", "persistentvolume",
        ]),
        ("admissionregistration.k8s.io", &[
            "mutatingwebhookconfigurations", "mutatingwebhookconfiguration",
            "validatingadmissionpolicies", "validatingadmissionpolicy",
            "validatingadmissionpolicybindings", "validatingadmissionpolicybinding",
            "validatingwebhookconfigurations", "validatingwebhookconfiguration",
        ]),
        ("apiextensions.k8s.io", &[
            "customresourcedefinitions", "customresourcedefinition",
        ]),
        ("apiregistration.k8s.io", &[
            "apiservices", "apiservice",
        ]),
        ("authentication.k8s.io", &[
            "selfsubjectreviews", "selfsubjectreview",
            "tokenreviews", "tokenreview",
        ]),
        ("authorization.k8s.io", &[
            "selfsubjectaccessreviews", "selfsubjectaccessreview",
            "selfsubjectrulesreviews", "selfsubjectrulesreview",
            "subjectaccessreviews", "subjectaccessreview",
        ]),
        ("certificates.k8s.io", &[
            "certificatesigningrequests", "certificatesigningrequest",
        ]),
        ("flowcontrol.apiserver.k8s.io", &[
            "flowschemas", "flowschema",
            "prioritylevelconfigurations", "prioritylevelconfiguration",
        ]),
        ("networking.k8s.io", &[
            "ingressclasses", "ingressclass",
            "ipaddresses", "ipaddress",
            "servicecidrs", "servicecidr",
        ]),
        ("node.k8s.io", &[
            "runtimeclasses", "runtimeclass",
        ]),
        ("rbac.authorization.k8s.io", &[
            "clusterrolebindings", "clusterrolebinding",
            "clusterroles", "clusterrole",
        ]),
        ("resource.k8s.io", &[
            "deviceclasses", "deviceclass",
            "resourceslices", "resourceslice",
        ]),
        ("scheduling.k8s.io", &[
            "priorityclasses", "priorityclass",
        ]),
        ("storage.k8s.io", &[
            "csidrivers", "csidriver",
            "csinodes", "csinode",
            "storageclasses", "storageclass",
            "volumeattachments", "volumeattachment",
            "volumeattributesclasses", "volumeattributesclass",
        ]),
    ];

    for (g, names) in cluster_scoped {
        if group == *g && names.contains(&name.as_str()) {
            return false; // cluster-scoped
        }
    }

    true // default: namespaced
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

    // --- sys.processes tests ---

    #[test]
    fn parse_process_line_parses_standard_ps_line() {
        // Format: user pid ppid cpu stime tty time cmd...
        // (ps -eo user,pid,ppid,c,stime,tty,time,cmd)
        let line = "root 649 1 0 20:28 pts/0 0:00 /usr/bin/bash --login";
        let proc = parse_process_line(line).expect("should parse");
        assert_eq!(proc.pid, 649);
        assert_eq!(proc.parent_pid, 1);
        assert_eq!(proc.user, Some("root".to_string()));
        assert_eq!(proc.start_time, Some("20:28".to_string()));
        assert_eq!(proc.name, "bash");
        assert_eq!(proc.cmd, "/usr/bin/bash --login");
    }

    #[test]
    fn parse_process_line_returns_none_on_too_few_fields() {
        assert!(parse_process_line("root 1 0").is_none());
    }

    #[test]
    fn parse_sys_processes_returns_known_failure_on_single_line() {
        let result = parse_sys_processes("USER PID PPID CPU START TTY TIME CMD", "");
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_sys_processes_parses_multiple_lines() {
        let stdout = "USER PID PPID CPU START TTY TIME CMD\n\
                      root 1 0 0 00:00 ? 0:00 /sbin/init\n\
                      root 649 1 0 20:28 pts/0 0:00 /usr/bin/bash";
        let result = parse_sys_processes(stdout, "");
        let ParserOutput::Success(updates, detail) = result else {
            panic!("expected Success");
        };
        assert_eq!(updates.processes.len(), 2);
        assert!(updates.processes.iter().any(|p| p.pid == 1));
        assert!(updates.processes.iter().any(|p| p.pid == 649));
        assert!(detail.contains("2 process"));
    }

    #[test]
    fn parse_sys_processes_unknown_format_on_bad_line() {
        let stdout = "USER PID PPID CPU START TTY TIME CMD\nnot-a-process-line";
        let result = parse_sys_processes(stdout, "");
        assert!(matches!(result, ParserOutput::UnknownFormat(_)));
    }

    // --- sys.userID tests ---

    #[test]
    fn parse_sys_userid_root_sets_root_exec() {
        let result = parse_sys_userid("uid=0(root) gid=0(root) groups=0(root)", "");
        let ParserOutput::Success(updates, detail) = result else {
            panic!("expected Success, got {:?}", result);
        };
        assert_eq!(updates.user_id, Some(0));
        assert_eq!(updates.username, Some("root".to_string()));
        assert_eq!(updates.access_level, Some(AccessLevel::RootExec));
        assert!(detail.contains("RootExec"));
    }

    #[test]
    fn parse_sys_userid_nonroot_sets_user_exec() {
        let result = parse_sys_userid("uid=1000(appuser) gid=1000(appuser) groups=1000(appuser)", "");
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success");
        };
        assert_eq!(updates.user_id, Some(1000));
        assert_eq!(updates.username, Some("appuser".to_string()));
        assert_eq!(updates.access_level, Some(AccessLevel::UserExec));
    }

    #[test]
    fn parse_sys_userid_bare_uid_no_username() {
        let result = parse_sys_userid("uid=500 gid=500", "");
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success");
        };
        assert_eq!(updates.user_id, Some(500));
        assert_eq!(updates.username, None);
        assert_eq!(updates.access_level, Some(AccessLevel::UserExec));
    }

    #[test]
    fn parse_sys_userid_empty_returns_known_failure() {
        let result = parse_sys_userid("", "");
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_output_effect_sys_userid_writes_access_level_to_entity() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.pods.insert(pod.entity_id(), pod);

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
        assert_eq!(sys.access_level, ran_domain::AccessLevel::RootExec);
    }

    // --- linux.mounts tests ---

    #[test]
    fn parse_mountinfo_line_parses_standard_line() {
        let line = "22 28 0:21 / /sys rw,nosuid,nodev shared:7 - sysfs sysfs rw";
        let m = parse_mountinfo_line(line).expect("should parse");
        assert_eq!(m.mount_point, "/sys");
        assert_eq!(m.mount_root, "/");
        assert_eq!(m.mount_type.as_deref(), Some("sysfs"));
        assert!(!m.is_host_path);
        assert!(!m.read_only);
    }

    #[test]
    fn parse_mountinfo_line_detects_kubelet_host_path() {
        let line = "256 255 8:1 / /var/lib/kubelet/pods/abc rw shared:12 - ext4 /dev/sda1 rw";
        let m = parse_mountinfo_line(line).expect("should parse");
        assert!(m.is_host_path);
    }

    #[test]
    fn parse_mount_cmd_line_parses_standard_line() {
        let line = "sysfs on /sys type sysfs (rw,nosuid,nodev,noexec,relatime)";
        let m = parse_mount_cmd_line(line).expect("should parse");
        assert_eq!(m.mount_point, "/sys");
        assert_eq!(m.mount_type.as_deref(), Some("sysfs"));
        assert!(!m.read_only);
    }

    #[test]
    fn parse_mount_cmd_line_detects_readonly() {
        let line = "/dev/sda1 on /mnt type ext4 (ro,relatime)";
        let m = parse_mount_cmd_line(line).expect("should parse");
        assert!(m.read_only);
    }

    #[test]
    fn parse_linux_mounts_parses_mixed_mountinfo_output() {
        let stdout = "\
22 28 0:21 / /sys rw shared:7 - sysfs sysfs rw\n\
36 28 8:1 / / rw shared:1 - ext4 /dev/sda1 rw\n\
256 255 8:1 /var/lib/kubelet /var/lib/kubelet rw shared:12 - ext4 /dev/sda1 rw\n";
        let result = parse_linux_mounts(stdout, "");
        let ParserOutput::Success(updates, detail) = result else {
            panic!("expected Success");
        };
        assert_eq!(updates.mounts.len(), 3);
        assert!(updates.mounts.iter().any(|m| m.is_host_path));
        assert!(detail.contains("3 mount"));
    }

    #[test]
    fn parse_linux_mounts_parses_mount_cmd_output() {
        let stdout = "\
sysfs on /sys type sysfs (rw,nosuid)\n\
/dev/sda1 on / type ext4 (rw,relatime)\n";
        let result = parse_linux_mounts(stdout, "");
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success");
        };
        assert_eq!(updates.mounts.len(), 2);
    }

    #[test]
    fn parse_linux_mounts_empty_returns_known_failure() {
        let result = parse_linux_mounts("", "");
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_output_effect_linux_mounts_writes_mounts_to_entity() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.pods.insert(pod.entity_id(), pod);

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
        campaign.pods.insert(pod.entity_id(), pod);

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

    // --- sys.has-binary tests ---

    #[test]
    fn parse_sys_has_binary_literal_path_derives_name_from_path() {
        let result = parse_sys_has_binary("", "/usr/bin/nmap");
        let ParserOutput::Success(updates, _) = result else { panic!("expected Success") };
        assert_eq!(updates.binaries.get("nmap").map(String::as_str), Some("/usr/bin/nmap"));
    }

    #[test]
    fn parse_sys_has_binary_bare_name_uses_name_as_path() {
        let result = parse_sys_has_binary("", "curl");
        let ParserOutput::Success(updates, _) = result else { panic!("expected Success") };
        assert_eq!(updates.binaries.get("curl").map(String::as_str), Some("curl"));
    }

    #[test]
    fn parse_sys_has_binary_two_arg_explicit_name() {
        // inner = "my-tool, /usr/local/bin/my-tool-v2"
        let result = parse_sys_has_binary("unused", "my-tool, /usr/local/bin/my-tool-v2");
        let ParserOutput::Success(updates, _) = result else { panic!("expected Success") };
        assert_eq!(
            updates.binaries.get("my-tool").map(String::as_str),
            Some("/usr/local/bin/my-tool-v2")
        );
    }

    #[test]
    fn parse_sys_has_binary_output_sentinel_extracts_paths_from_stdout() {
        let stdout = "/usr/bin/redis-benchmark\n/usr/bin/redis-cli\ndebconf: noise line\n";
        let result = parse_sys_has_binary(stdout, "${OUTPUT}");
        let ParserOutput::Success(updates, _) = result else { panic!("expected Success") };
        assert_eq!(updates.binaries.get("redis-benchmark").map(String::as_str), Some("/usr/bin/redis-benchmark"));
        assert_eq!(updates.binaries.get("redis-cli").map(String::as_str), Some("/usr/bin/redis-cli"));
    }

    #[test]
    fn parse_sys_has_binary_output_sentinel_empty_stdout_returns_known_failure() {
        let result = parse_sys_has_binary("", "${OUTPUT}");
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_output_effect_has_binary_writes_to_entity() {
        let mut campaign = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("dev"));
        let pod = Pod::new("demo", "default");
        campaign.pods.insert(pod.entity_id(), pod);

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

    // --- rawServiceAccountToken tests ---

    /// Build a minimal JWT string with the given JSON payload (no real signature).
    fn make_jwt(payload_json: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(payload_json);
        format!("{}.{}.fakesig", header, payload)
    }

    #[test]
    fn parse_raw_sa_token_projected_creates_entities_and_relations() {
        let payload = r#"{
            "aud": ["https://kubernetes.default.svc.cluster.local"],
            "exp": 9999999999,
            "iat": 1000000000,
            "iss": "https://kubernetes.default.svc.cluster.local",
            "kubernetes.io": {
                "namespace": "prod",
                "node": {"name": "worker-1", "uid": "node-uid-1"},
                "pod": {"name": "api-pod", "uid": "pod-uid-1"},
                "serviceaccount": {"name": "api-sa", "uid": "sa-uid-1"}
            },
            "sub": "system:serviceaccount:prod:api-sa"
        }"#;
        let jwt = make_jwt(payload);
        let result = parse_raw_service_account_token(&jwt, "");

        let ParserOutput::SuccessWithFacts(facts, detail) = result else {
            panic!("expected SuccessWithFacts, got {:?}", result);
        };

        assert!(detail.contains("prod/api-sa"));

        // Namespace, ServiceAccount, Pod, K8sNode
        assert_eq!(facts.new_entities.len(), 4);
        assert!(facts.new_entities.iter().any(|e| e.entity_kind() == "Namespace" && e.entity_name() == "prod"));
        assert!(facts.new_entities.iter().any(|e| e.entity_kind() == "ServiceAccount" && e.entity_name() == "api-sa"));
        assert!(facts.new_entities.iter().any(|e| e.entity_kind() == "Pod" && e.entity_name() == "api-pod"));
        assert!(facts.new_entities.iter().any(|e| e.entity_kind() == "Node" && e.entity_name() == "worker-1"));

        // Contains (ns→sa), Uses (pod→sa), RunsOn (pod→node)
        assert_eq!(facts.new_relations.len(), 3);
        assert!(facts.new_relations.iter().any(|r| r.is::<Contains>()));
        assert!(facts.new_relations.iter().any(|r| r.is::<Uses>()));
        assert!(facts.new_relations.iter().any(|r| r.is::<RunsOn>()));
    }

    #[test]
    fn parse_raw_sa_token_legacy_creates_entities_without_node() {
        let payload = r#"{
            "iss": "kubernetes/serviceaccount",
            "kubernetes.io/serviceaccount/namespace": "default",
            "kubernetes.io/serviceaccount/service-account.name": "default-sa",
            "kubernetes.io/serviceaccount/service-account.uid": "abc123",
            "sub": "system:serviceaccount:default:default-sa"
        }"#;
        let jwt = make_jwt(payload);
        let result = parse_raw_service_account_token(&jwt, "");

        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };

        assert!(facts.new_entities.iter().any(|e| e.entity_kind() == "ServiceAccount" && e.entity_name() == "default-sa"));
        // No node entity since legacy tokens don't carry node info.
        assert!(!facts.new_entities.iter().any(|e| e.entity_kind() == "Node"));
        // No RunsOn relation.
        assert!(!facts.new_relations.iter().any(|r| r.is::<RunsOn>()));
    }

    #[test]
    fn parse_raw_sa_token_token_is_set_on_sa_entity() {
        let payload = r#"{
            "kubernetes.io": {
                "namespace": "kube-system",
                "pod": {"name": "coredns", "uid": "uid-1"},
                "serviceaccount": {"name": "coredns"}
            },
            "sub": "system:serviceaccount:kube-system:coredns"
        }"#;
        let jwt = make_jwt(payload);
        let result = parse_raw_service_account_token(&jwt, "");

        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };

        let sa_entity = facts
            .new_entities
            .iter()
            .find(|e| e.entity_kind() == "ServiceAccount")
            .expect("SA entity must be present");

        let sa = sa_entity
            .as_any()
            .downcast_ref::<ran_domain::ServiceAccount>()
            .expect("must downcast to ServiceAccount");

        let token = sa.token.as_ref().expect("token must be set on SA");
        assert!(!token.raw().is_empty(), "raw JWT must be stored in token");
        assert_eq!(token.service_account_name, "coredns");
        assert_eq!(token.namespace, "kube-system");
        assert!(token.is_bound, "pod uid present → token is bound");
    }

    #[test]
    fn parse_raw_sa_token_multiline_output_finds_jwt() {
        let payload = r#"{"kubernetes.io":{"namespace":"test","serviceaccount":{"name":"test-sa"}},"sub":"system:serviceaccount:test:test-sa"}"#;
        let jwt = make_jwt(payload);
        // Wrap the token in noisy multi-line output.
        let stdout = format!("some noise\n{jwt}\nmore noise\n");
        let result = parse_raw_service_account_token(&stdout, "");
        assert!(matches!(result, ParserOutput::SuccessWithFacts(_, _)));
    }

    #[test]
    fn parse_raw_sa_token_empty_input_returns_known_failure() {
        let result = parse_raw_service_account_token("", "");
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_raw_sa_token_invalid_base64_returns_unknown_format() {
        let result = parse_raw_service_account_token("eyXXX.!!!.sig", "");
        assert!(matches!(result, ParserOutput::UnknownFormat(_)));
    }

    #[test]
    fn parse_raw_sa_token_missing_k8s_claims_returns_unknown_format() {
        // Valid JWT but payload has no kubernetes claims.
        let payload = r#"{"sub": "some-subject", "exp": 99999}"#;
        let jwt = make_jwt(payload);
        let result = parse_raw_service_account_token(&jwt, "");
        assert!(matches!(result, ParserOutput::UnknownFormat(_)));
    }

    // -------------------------------------------------------------------------
    // parse_rdns
    // -------------------------------------------------------------------------

    #[test]
    fn parse_rdns_valid_pod_entries() {
        let stdout = "ip,ptr\n\
            10.244.1.4,10-244-1-4.backend-service.dev.svc.cluster.local\n\
            10.244.1.6,10-244-1-6.argocd-notifications-controller-metrics.argocd.svc.cluster.local\n\
            192.168.0.5,host.local\n";
        let result = parse_rdns(stdout, "");
        let ParserOutput::SuccessWithFacts(facts, detail) = result else {
            panic!("expected SuccessWithFacts, got {:?}", result);
        };
        assert_eq!(facts.new_entities.len(), 2, "should discover 2 pods");
        assert!(facts.new_entities.iter().any(|e| e.entity_kind() == "Pod" && e.entity_name() == "backend-service.10-244-1-4"));
        assert!(facts.new_entities.iter().any(|e| e.entity_kind() == "Pod" && e.entity_name() == "argocd-notifications-controller-metrics.10-244-1-6"));
        assert!(detail.contains("2 pod"));
    }

    #[test]
    fn parse_rdns_pod_has_ip_set() {
        let stdout = "10.244.1.4,10-244-1-4.backend-service.dev.svc.cluster.local\n";
        let ParserOutput::SuccessWithFacts(facts, _) = parse_rdns(stdout, "") else {
            panic!("expected SuccessWithFacts");
        };
        let pod = facts.new_entities.iter().find(|e| e.entity_kind() == "Pod").unwrap();
        let pod = pod.as_any().downcast_ref::<Pod>().unwrap();
        assert!(pod.system.ips.iter().any(|ip| ip.to_string() == "10.244.1.4"));
        assert_eq!(pod.namespace().unwrap(), "dev");
    }

    #[test]
    fn parse_rdns_skips_non_cluster_local() {
        let stdout = "ip,ptr\n192.168.0.5,host.local\n10.0.0.1,internal.example.com\n";
        let result = parse_rdns(stdout, "");
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_rdns_empty_input() {
        let result = parse_rdns("", "");
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_rdns_header_only() {
        let result = parse_rdns("ip,ptr\n", "");
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_rdns_skips_invalid_lines() {
        let stdout = "ip,ptr\n\
            not-an-ip,some.cluster.local\n\
            this-is-not-csv\n\
            10.0.0.1,10-0-0-1.backend.default.svc.cluster.local\n";
        let result = parse_rdns(stdout, "");
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 1);
    }

    #[test]
    fn parse_rdns_without_header() {
        let stdout = "10.244.1.4,10-244-1-4.backend-service.dev.svc.cluster.local\n\
            10.244.1.6,10-244-1-6.argocd-server.argocd.svc.cluster.local\n";
        let result = parse_rdns(stdout, "");
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 2);
    }
}
