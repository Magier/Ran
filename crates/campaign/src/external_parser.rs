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
        if !sys.mounts.iter().any(|m| m.mount_point == mount.mount_point) {
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
        // Only record if currently unknown — the parser has definitive data, but we
        // don't want to silently overwrite a more precise path already recorded.
        if sys.has_binary(name) == BinaryPresence::Unknown {
            sys.set_binary(name.clone(), path.clone());
            count += 1;
        }
    }

    count
}
