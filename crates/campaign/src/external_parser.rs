//! External (script-based) parser interface.
//!
//! When a compiled output parser does not exist for a TTP effect, the runtime
//! can delegate to an external process.  The [`ExternalParser`] trait abstracts
//! the transport; [`ScriptParserRunner`] (in the CLI crate) is the concrete
//! implementation that looks up a script on disk and executes it.
//!
//! ## Protocol
//!
//! **Input** (JSON on stdin):
//! ```json
//! {
//!   "effect_id": "sys.ip",
//!   "ttp_id":    "read-ips",
//!   "target_id": "ns/default/pod/demo",
//!   "exec_system_id": "",
//!   "args":      { "NAMESPACE": "default" },
//!   "results":   ["10.244.0.5\n192.168.1.100", ""],
//!   "exit_code": 0,
//!   "success":   true
//! }
//! ```
//!
//! **Output** (JSON on stdout):
//! ```json
//! {
//!   "system": {
//!     "ips": ["10.244.0.5", "192.168.1.100"],
//!     "env_vars": { "K": "V" },
//!     "files": ["/etc/passwd"]
//!   },
//!   "detail": "parsed 2 IP addresses"
//! }
//! ```
//!
//! Only the fields present in the response are merged; omitted fields are left
//! untouched.

use std::collections::HashMap;
use std::net::IpAddr;

use ran_domain::BinaryPresence;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Context sent to the external parser.
#[derive(Debug, Clone, Serialize)]
pub struct ExternalParseRequest {
    pub effect_id: String,
    pub ttp_id: String,
    pub target_id: String,
    pub exec_system_id: String,
    pub args: HashMap<String, String>,
    pub results: Vec<String>,
    pub exit_code: i32,
    pub success: bool,
}

/// Response returned by the external parser.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExternalParseResponse {
    /// Partial system-info updates to merge into the target entity.
    #[serde(default)]
    pub system: SystemFieldUpdates,

    /// Human-readable detail for the parse audit entry.
    #[serde(default)]
    pub detail: String,
}

/// A bag of optional system-info fields.  Only populated fields are merged.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SystemFieldUpdates {
    #[serde(default)]
    pub ips: Vec<String>,

    #[serde(default)]
    pub env_vars: HashMap<String, String>,

    #[serde(default)]
    pub files: Vec<String>,

    #[serde(default)]
    pub os: Option<String>,

    #[serde(default)]
    pub username: Option<String>,

    #[serde(default)]
    pub user_id: Option<u32>,

    #[serde(default)]
    pub processes: Vec<ran_domain::Process>,

    #[serde(default)]
    pub mounts: Vec<ran_domain::Mount>,

    #[serde(default)]
    pub access_level: Option<ran_domain::AccessLevel>,

    /// Binary name → path on the system. Empty path means the binary is known to be absent.
    #[serde(default)]
    pub binaries: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// An async callback invoked for effects without a compiled parser.
#[async_trait::async_trait]
pub trait ExternalParser: Send + Sync + 'static {
    /// Try to parse the given effect.  Return `None` if no external parser is
    /// available for this effect either.
    async fn try_parse(&self, request: ExternalParseRequest) -> Option<ExternalParseResponse>;
}

// ---------------------------------------------------------------------------
// Merge helpers
// ---------------------------------------------------------------------------

/// Apply the script's response to a `SystemInfo`.  Returns the number of new
/// facts written.
pub fn apply_system_field_updates(
    sys: &mut ran_domain::SystemInfo,
    updates: &SystemFieldUpdates,
) -> usize {
    let mut count = 0usize;

    for ip_str in &updates.ips {
        if let Ok(ip) = ip_str.parse::<IpAddr>() {
            if !sys.ips.contains(&ip) {
                sys.ips.push(ip);
                count += 1;
            }
        }
    }

    for (k, v) in &updates.env_vars {
        if sys.env_vars.get(k) != Some(v) {
            sys.env_vars.insert(k.clone(), v.clone());
            count += 1;
        }
    }

    for f in &updates.files {
        if !sys.files.contains(f) {
            sys.files.push(f.clone());
            count += 1;
        }
    }

    if let Some(os) = &updates.os {
        if sys.os.as_deref() != Some(os) {
            sys.os = Some(os.clone());
            count += 1;
        }
    }

    if let Some(username) = &updates.username {
        if sys.username.as_deref() != Some(username) {
            sys.username = Some(username.clone());
            count += 1;
        }
    }

    if let Some(uid) = updates.user_id {
        if sys.user_id != Some(uid) {
            sys.user_id = Some(uid);
            count += 1;
        }
    }

    for proc in &updates.processes {
        if !sys.processes.iter().any(|p| p.pid == proc.pid) {
            sys.processes.push(proc.clone());
            count += 1;
        }
    }

    for mount in &updates.mounts {
        if !sys
            .mounts
            .iter()
            .any(|m| m.mount_point == mount.mount_point)
        {
            sys.mounts.push(mount.clone());
            count += 1;
        }
    }

    if let Some(level) = &updates.access_level {
        if &sys.access_level != level {
            sys.access_level = *level;
            count += 1;
        }
    }

    for (name, path) in &updates.binaries {
        let should_write = match sys.has_binary(name) {
            BinaryPresence::Unknown => true,
            BinaryPresence::Absent => !path.is_empty(),
            BinaryPresence::Present(existing) => {
                // A command-not-found result is direct, current evidence that a
                // previously discovered binary is no longer available. Keep
                // this reversible: a later non-empty update (for example after
                // installing the package) replaces Absent above.
                path.is_empty() || is_more_precise_binary_path(&existing, path)
            }
        };

        if should_write {
            sys.set_binary(name.clone(), path.clone());
            count += 1;
        }
    }

    count
}

fn is_more_precise_binary_path(existing: &str, candidate: &str) -> bool {
    if existing == candidate {
        return false;
    }
    let existing_abs = existing.starts_with('/');
    let candidate_abs = candidate.starts_with('/');
    match (existing_abs, candidate_abs) {
        (false, true) => true,
        (true, false) => false,
        // If both are absolute, keep the existing one to avoid churn.
        // If both are non-absolute, don't rewrite either.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_update_upgrades_name_to_absolute_path() {
        let mut sys = ran_domain::SystemInfo::default();
        sys.set_binary("nmap", "nmap");

        let updates = SystemFieldUpdates {
            binaries: HashMap::from([("nmap".to_string(), "/usr/bin/nmap".to_string())]),
            ..Default::default()
        };

        let changed = apply_system_field_updates(&mut sys, &updates);
        assert_eq!(changed, 1);
        assert_eq!(
            sys.has_binary("nmap"),
            ran_domain::BinaryPresence::Present("/usr/bin/nmap".to_string())
        );
    }

    #[test]
    fn binary_update_does_not_downgrade_absolute_path_to_name() {
        let mut sys = ran_domain::SystemInfo::default();
        sys.set_binary("nmap", "/usr/bin/nmap");

        let updates = SystemFieldUpdates {
            binaries: HashMap::from([("nmap".to_string(), "nmap".to_string())]),
            ..Default::default()
        };

        let changed = apply_system_field_updates(&mut sys, &updates);
        assert_eq!(changed, 0);
        assert_eq!(
            sys.has_binary("nmap"),
            ran_domain::BinaryPresence::Present("/usr/bin/nmap".to_string())
        );
    }

    #[test]
    fn binary_update_keeps_absent_when_negative_fact_already_recorded() {
        let mut sys = ran_domain::SystemInfo::default();
        sys.set_binary("nmap", "");

        let updates = SystemFieldUpdates {
            binaries: HashMap::from([("nmap".to_string(), "/usr/bin/nmap".to_string())]),
            ..Default::default()
        };

        let changed = apply_system_field_updates(&mut sys, &updates);
        assert_eq!(changed, 1);
        assert_eq!(
            sys.has_binary("nmap"),
            ran_domain::BinaryPresence::Present("/usr/bin/nmap".to_string())
        );
    }

    #[test]
    fn binary_update_replaces_stale_presence_with_negative_evidence() {
        let mut sys = ran_domain::SystemInfo::default();
        sys.set_binary("wget", "/usr/bin/wget");

        let updates = SystemFieldUpdates {
            binaries: HashMap::from([("wget".to_string(), String::new())]),
            ..Default::default()
        };

        let changed = apply_system_field_updates(&mut sys, &updates);
        assert_eq!(changed, 1);
        assert_eq!(sys.has_binary("wget"), ran_domain::BinaryPresence::Absent);
    }
}
