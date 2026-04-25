use std::collections::HashMap;
use std::net::IpAddr;

use ran_domain::{AccessLevel, Mount};

use super::ParserOutput;
use crate::external_parser::SystemFieldUpdates;

pub(super) fn register(m: &mut HashMap<&'static str, super::ParserFn>) {
    m.insert("sys.envvar", parse_sys_envvar);
    m.insert("sys.ip", parse_sys_ip);
    m.insert("sys.processes", parse_sys_processes);
    m.insert("sys.userid", parse_sys_userid);
    m.insert("sys.files", parse_sys_files);
    m.insert("linux.mounts", parse_linux_mounts);
}

pub(super) fn parse_sys_has_binary(stdout: &str, inner: &str) -> ParserOutput {
    let (explicit_name, source) = split_has_binary_args(inner);
    let is_output =
        source.eq_ignore_ascii_case("${output}") || source.eq_ignore_ascii_case("output");

    let binaries: HashMap<String, String> = if is_output {
        let paths = parse_binary_paths_from_output(stdout);
        if paths.is_empty() {
            return ParserOutput::KnownFailure("no binary paths found in stdout".to_string());
        }
        paths
            .into_iter()
            .map(|path| {
                let name = explicit_name
                    .clone()
                    .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(&path).to_string());
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

/// Parse a line-delimited file listing.
///
/// Handles two input formats:
/// - `find`-style: one absolute path per line (optionally ending in `*` for executables)
/// - `ls -l`-style: long listing with permissions, size, date, and name in field 8
///
/// Executable entries (filename ending in `*` via `ls -F`) are also recorded in
/// `system.binaries` with an empty path sentinel (name known, path not resolvable).
fn parse_sys_files(stdout: &str, _stderr: &str, args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty output from file listing command".to_string());
    }

    let mut files = Vec::new();
    let mut binaries: HashMap<String, String> = HashMap::new();
    let ls_long = is_ls_long_format(stdout);
    let dir = args
        .get("DIR")
        .map(String::as_str)
        .unwrap_or("")
        .trim_end_matches('/');

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("total ") {
            continue;
        }

        let raw = if ls_long {
            match extract_ls_long_name(line) {
                Some(n) => n,
                None => continue,
            }
        } else {
            line
        };

        // Skip . and .. directory entries produced by ls -a
        let base = raw.trim_end_matches(|c| matches!(c, '/' | '@' | '=' | '|' | '>'));
        if base == "." || base == ".." {
            continue;
        }

        // Lines ending in `*` mark executables (ls -F / find -perm /111 style).
        let (name, is_exec) = if let Some(stripped) = raw.strip_suffix('*') {
            (stripped.trim(), true)
        } else {
            (base, false)
        };

        if name.is_empty() {
            continue;
        }

        // Construct the full path: use the name as-is if it's already absolute
        // or if no directory context is available; otherwise prepend DIR.
        let path = if name.starts_with('/') || dir.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", dir, name)
        };

        files.push(path.clone());
        if is_exec {
            let bin_name = path.rsplit('/').next().unwrap_or(&path).to_string();
            binaries.entry(bin_name).or_insert_with(|| path.clone());
        }
    }

    let detail = format!("recorded {} file(s)", files.len());
    ParserOutput::Success(
        SystemFieldUpdates {
            files,
            binaries,
            ..Default::default()
        },
        detail,
    )
}

/// Detect whether stdout looks like `ls -l` long-listing format.
///
/// Heuristic: at least one of the first ten non-empty lines starts with "total "
/// or has a 10-character permission field (e.g. `-rwxr-xr-x`).
fn is_ls_long_format(stdout: &str) -> bool {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(10)
        .any(|l| {
            l.starts_with("total ")
                || (l.len() >= 10
                    && matches!(
                        l.chars().next(),
                        Some('-' | 'd' | 'l' | 'b' | 'c' | 'p' | 's')
                    )
                    && l.split_whitespace().count() >= 9)
        })
}

/// Extract the filename from an `ls -l` line.
///
/// Format: `permissions links owner group size month day time name [-> target]`
/// The filename is field index 8 (0-based). For symlinks (`name -> target`) only
/// the source name is returned.
fn extract_ls_long_name(line: &str) -> Option<&str> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 9 {
        return None;
    }
    // Validate that field 0 looks like a permission string (length 10).
    if fields[0].len() != 10 {
        return None;
    }
    let name = fields[8];
    // Strip symlink " -> target" suffix by returning only up to the first space
    // (fields[8] from split_whitespace already excludes spaces, so name is clean).
    Some(name)
}

/// Parametric effect: `sys.hasfile(PATH)`.
///
/// If stdout is non-empty or exit code is 0 the file is marked present in
/// `system.files`.  If stdout is empty (command returned nothing / failed)
/// the file is considered absent — we emit `KnownFailure` so the audit
/// records the negative result without triggering a `NoParser` signal.
pub(super) fn parse_sys_hasfile(stdout: &str, path: &str) -> ParserOutput {
    let path = path.trim();
    if path.is_empty() {
        return ParserOutput::KnownFailure(
            "sys.hasfile effect had empty path argument".to_string(),
        );
    }

    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure(format!(
            "file '{}' not found or command returned no output",
            path
        ));
    }

    ParserOutput::Success(
        SystemFieldUpdates {
            files: vec![path.to_string()],
            ..Default::default()
        },
        format!("file '{}' confirmed present", path),
    )
}

fn parse_sys_envvar(stdout: &str, stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
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

fn parse_sys_ip(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
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

fn parse_sys_processes(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
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
fn parse_process_line(line: &str) -> Option<ran_domain::Process> {
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

    Some(ran_domain::Process {
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
/// Sets `access_level` to `Exec` for any uid.
fn parse_sys_userid(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
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

    let access_level = AccessLevel::Exec;

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
fn parse_linux_mounts(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
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
        return ParserOutput::UnknownFormat("no mount entries recognised in output".to_string());
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
        mount_type: if fs_type.is_empty() {
            None
        } else {
            Some(fs_type)
        },
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
        mount_type: if fs_type.is_empty() {
            None
        } else {
            Some(fs_type)
        },
        read_only,
        is_host_path,
    })
}

/// Returns `true` when the mount point is a kubelet-managed host path,
/// indicating the pod has visibility into the node's filesystem.
fn is_kubelet_host_path(mount_point: &str) -> bool {
    mount_point.contains("/var/lib/kubelet")
}

/// Extract the text between the outermost `(` and `)` of an effect string.
pub(super) fn extract_effect_args(effect: &str) -> Option<&str> {
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
        let name_part = inner[..comma_pos]
            .trim()
            .trim_matches(|c| c == '\'' || c == '"');
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

pub(super) fn parse_ip_addrs(stdout: &str) -> Vec<IpAddr> {
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

pub(super) fn parse_env_vars(stdout: &str) -> HashMap<String, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ran_domain::AccessLevel;

    #[test]
    fn parses_standard_env_output_fixture() {
        let stdout_fixture =
            "HOME=/root\nPATH=/usr/local/sbin:/usr/local/bin\nKUBERNETES_SERVICE_HOST=10.96.0.1\n";
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
    fn parse_ip_addrs_parses_mixed_ipv4_and_ipv6() {
        let ips = parse_ip_addrs("10.0.0.1 192.168.1.5 ::1");
        assert_eq!(ips.len(), 3);
        assert!(ips.iter().any(|ip| ip.to_string() == "10.0.0.1"));
        assert!(ips.iter().any(|ip| ip.to_string() == "192.168.1.5"));
        assert!(ips.iter().any(|ip| ip.to_string() == "::1"));
    }

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
        let result = parse_sys_processes("USER PID PPID CPU START TTY TIME CMD", "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_sys_processes_parses_multiple_lines() {
        let stdout = "USER PID PPID CPU START TTY TIME CMD\n\
                      root 1 0 0 00:00 ? 0:00 /sbin/init\n\
                      root 649 1 0 20:28 pts/0 0:00 /usr/bin/bash";
        let result = parse_sys_processes(stdout, "", &HashMap::new());
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
        let result = parse_sys_processes(stdout, "", &HashMap::new());
        assert!(matches!(result, ParserOutput::UnknownFormat(_)));
    }

    #[test]
    fn parse_sys_userid_root_sets_root_exec() {
        let result = parse_sys_userid("uid=0(root) gid=0(root) groups=0(root)", "", &HashMap::new());
        let ParserOutput::Success(updates, detail) = result else {
            panic!("expected Success, got {:?}", result);
        };
        assert_eq!(updates.user_id, Some(0));
        assert_eq!(updates.username, Some("root".to_string()));
        assert_eq!(updates.access_level, Some(AccessLevel::Exec));
        assert!(detail.contains("Exec"));
    }

    #[test]
    fn parse_sys_userid_nonroot_sets_user_exec() {
        let result = parse_sys_userid(
            "uid=1000(appuser) gid=1000(appuser) groups=1000(appuser)",
            "",
            &HashMap::new(),
        );
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success");
        };
        assert_eq!(updates.user_id, Some(1000));
        assert_eq!(updates.username, Some("appuser".to_string()));
        assert_eq!(updates.access_level, Some(AccessLevel::Exec));
    }

    #[test]
    fn parse_sys_userid_bare_uid_no_username() {
        let result = parse_sys_userid("uid=500 gid=500", "", &HashMap::new());
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success");
        };
        assert_eq!(updates.user_id, Some(500));
        assert_eq!(updates.username, None);
        assert_eq!(updates.access_level, Some(AccessLevel::Exec));
    }

    #[test]
    fn parse_sys_userid_empty_returns_known_failure() {
        let result = parse_sys_userid("", "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

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
        let result = parse_linux_mounts(stdout, "", &HashMap::new());
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
        let result = parse_linux_mounts(stdout, "", &HashMap::new());
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success");
        };
        assert_eq!(updates.mounts.len(), 2);
    }

    #[test]
    fn parse_linux_mounts_empty_returns_known_failure() {
        let result = parse_linux_mounts("", "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_sys_has_binary_literal_path_derives_name_from_path() {
        let result = parse_sys_has_binary("", "/usr/bin/nmap");
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success")
        };
        assert_eq!(
            updates.binaries.get("nmap").map(String::as_str),
            Some("/usr/bin/nmap")
        );
    }

    #[test]
    fn parse_sys_has_binary_bare_name_uses_name_as_path() {
        let result = parse_sys_has_binary("", "curl");
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success")
        };
        assert_eq!(
            updates.binaries.get("curl").map(String::as_str),
            Some("curl")
        );
    }

    #[test]
    fn parse_sys_has_binary_two_arg_explicit_name() {
        // inner = "my-tool, /usr/local/bin/my-tool-v2"
        let result = parse_sys_has_binary("unused", "my-tool, /usr/local/bin/my-tool-v2");
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success")
        };
        assert_eq!(
            updates.binaries.get("my-tool").map(String::as_str),
            Some("/usr/local/bin/my-tool-v2")
        );
    }

    #[test]
    fn parse_sys_has_binary_output_sentinel_extracts_paths_from_stdout() {
        let stdout = "/usr/bin/redis-benchmark\n/usr/bin/redis-cli\ndebconf: noise line\n";
        let result = parse_sys_has_binary(stdout, "${OUTPUT}");
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success")
        };
        assert_eq!(
            updates.binaries.get("redis-benchmark").map(String::as_str),
            Some("/usr/bin/redis-benchmark")
        );
        assert_eq!(
            updates.binaries.get("redis-cli").map(String::as_str),
            Some("/usr/bin/redis-cli")
        );
    }

    #[test]
    fn parse_sys_has_binary_output_sentinel_empty_stdout_returns_known_failure() {
        let result = parse_sys_has_binary("", "${OUTPUT}");
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    // --- sys.files ---

    #[test]
    fn parse_sys_files_multi_line_listing_populates_files() {
        let stdout = "/etc/passwd\n/etc/hostname\n/tmp/app.conf\n";
        let result = parse_sys_files(stdout, "", &HashMap::new());
        let ParserOutput::Success(updates, detail) = result else {
            panic!("expected Success, got {:?}", result);
        };
        assert_eq!(updates.files.len(), 3);
        assert!(updates.files.contains(&"/etc/passwd".to_string()));
        assert!(updates.files.contains(&"/tmp/app.conf".to_string()));
        assert!(detail.contains("3 file"));
    }

    #[test]
    fn parse_sys_files_executable_marker_records_binary() {
        let stdout = "/usr/bin/curl*\n/etc/passwd\n";
        let result = parse_sys_files(stdout, "", &HashMap::new());
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success");
        };
        // The stripped path (without `*`) should be in files.
        assert!(updates.files.contains(&"/usr/bin/curl".to_string()));
        // The binary name maps to its full absolute path.
        assert_eq!(updates.binaries.get("curl").map(String::as_str), Some("/usr/bin/curl"));
        // Non-executable path should not be in binaries.
        assert!(!updates.binaries.contains_key("passwd"));
    }

    #[test]
    fn parse_sys_files_empty_output_returns_known_failure() {
        let result = parse_sys_files("", "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_sys_files_ls_long_format_extracts_filenames() {
        let stdout = "\
total 65644\n\
drwxrwxrwt 1 root root     4096 Apr 25 07:41 ./\n\
drwxr-xr-x 1 root root     4096 Apr 25 06:09 ../\n\
-rwxr-xr-x 1 root root 59502754 Apr 25 07:41 kubectl*\n\
-rwxr-xr-x 1 root root  7697200 Apr 25 07:41 helm*\n\
-rw-r--r-- 1 root root     1024 Apr 25 07:00 config.yaml\n";
        // Without DIR the filename is used as-is.
        let result = parse_sys_files(stdout, "", &HashMap::new());
        let ParserOutput::Success(updates, detail) = result else {
            panic!("expected Success, got {:?}", result);
        };
        assert_eq!(updates.files.len(), 3, "files: {:?}", updates.files);
        assert!(updates.files.contains(&"kubectl".to_string()));
        assert!(updates.files.contains(&"config.yaml".to_string()));
        assert!(updates.binaries.contains_key("kubectl"));
        assert_eq!(updates.binaries["kubectl"], "kubectl");
        assert!(!updates.binaries.contains_key("config.yaml"));
        assert!(detail.contains("3 file"));
    }

    #[test]
    fn parse_sys_files_ls_long_with_dir_constructs_full_paths() {
        let stdout = "\
total 65644\n\
drwxrwxrwt 1 root root     4096 Apr 25 07:41 ./\n\
drwxr-xr-x 1 root root     4096 Apr 25 06:09 ../\n\
-rwxr-xr-x 1 root root 59502754 Apr 25 07:41 kubectl*\n\
-rw-r--r-- 1 root root     1024 Apr 25 07:00 config.yaml\n";
        let mut args = HashMap::new();
        args.insert("DIR".to_string(), "/tmp".to_string());
        let result = parse_sys_files(stdout, "", &args);
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success, got {:?}", result);
        };
        assert!(updates.files.contains(&"/tmp/kubectl".to_string()));
        assert!(updates.files.contains(&"/tmp/config.yaml".to_string()));
        assert_eq!(updates.binaries.get("kubectl").map(String::as_str), Some("/tmp/kubectl"));
    }

    #[test]
    fn parse_sys_files_ls_long_skips_dot_entries() {
        let stdout = "\
total 0\n\
drwxr-xr-x 2 root root 40 Apr 25 07:00 ./\n\
drwxr-xr-x 3 root root 60 Apr 25 07:00 ../\n\
-rw-r--r-- 1 root root  0 Apr 25 07:00 empty.txt\n";
        let result = parse_sys_files(stdout, "", &HashMap::new());
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success");
        };
        assert_eq!(updates.files, vec!["empty.txt".to_string()]);
    }

    // --- sys.hasfile ---

    #[test]
    fn parse_sys_hasfile_exit0_marks_file_present() {
        let result = parse_sys_hasfile("exists", "/etc/passwd");
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success, got {:?}", result);
        };
        assert!(updates.files.contains(&"/etc/passwd".to_string()));
    }

    #[test]
    fn parse_sys_hasfile_empty_stdout_returns_known_failure() {
        let result = parse_sys_hasfile("", "/etc/shadow");
        assert!(
            matches!(result, ParserOutput::KnownFailure(_)),
            "absent file should yield KnownFailure"
        );
    }

    #[test]
    fn parse_sys_hasfile_path_extracted_from_effect_id_with_nested_separators() {
        // Path contains `/` — the extraction from the effect string must not split on them.
        let result = parse_sys_hasfile(
            "found",
            "/var/run/secrets/kubernetes.io/serviceaccount/token",
        );
        let ParserOutput::Success(updates, _) = result else {
            panic!("expected Success");
        };
        assert!(updates
            .files
            .contains(&"/var/run/secrets/kubernetes.io/serviceaccount/token".to_string()));
    }

    #[test]
    fn parse_sys_hasfile_empty_path_argument_returns_known_failure() {
        let result = parse_sys_hasfile("some output", "");
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }
}
