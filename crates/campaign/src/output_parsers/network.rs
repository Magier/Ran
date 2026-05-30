use std::collections::HashMap;
use std::net::IpAddr;

use ran_domain::{CanReach, Entity, Pod};

use super::ParserOutput;
use crate::FactsUpdate;

pub(super) fn register(m: &mut HashMap<&'static str, super::ParserFn>) {
    m.insert("rdns", parse_rdns);
}

/// Parser for the `nmap` effect — network host discovery.
///
/// Accepts two output formats produced by nmap:
///
/// **Greppable (`-oG`)** — lines starting with `Host:` where `Status: Up` or
/// a `Ports:` field is present:
/// ```text
/// Host: 10.0.0.5 ()           Status: Up
/// Host: 10.0.0.6 (redis.default.svc.cluster.local)   Ports: 6379/open/tcp
/// Host: 10.0.0.7 ()           Status: Down
/// ```
///
/// **XML (`-oX`)** — detected by `<?xml` or `<nmaprun` prefix; parses `<host>`
/// elements with an `<address addrtype="ipv4">` child and optional
/// `<hostname type="PTR">` child.
///
/// For each live host a placeholder `Pod` entity is emitted named
/// `pod-<ip-kebab>` (e.g. `10.0.0.5` → `pod-10-0-0-5`) with the IP recorded
/// in `system.ips`.  A `CanReach` relation is also emitted from `source_id`
/// (the scanning pod) to each placeholder.
///
/// `source_id` is passed explicitly because the registry `ParserFn` type only
/// carries `(stdout, stderr)` — the caller in `parse_output_effect` resolves
/// it from `cmd.target_id`.
pub(super) fn parse_nmap(stdout: &str, source_id: &str) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty nmap output".to_string());
    }

    let hosts = if stdout.contains("<nmaprun") || stdout.trim_start().starts_with("<?xml") {
        parse_nmap_xml(stdout)
    } else if stdout.contains("Host:") {
        parse_nmap_grep(stdout)
    } else {
        return ParserOutput::UnknownFormat(
            "nmap output is neither greppable (-oG) nor XML (-oX) format".to_string(),
        );
    };

    if hosts.is_empty() {
        return ParserOutput::KnownFailure("no live hosts discovered in nmap output".to_string());
    }

    let mut facts = FactsUpdate::default();
    for (ip, hostname) in &hosts {
        let ip_kebab = ip.replace('.', "-");
        let pod_name = format!("pod-{}", ip_kebab);

        // Use hostname as pod name when available, so entities can merge with
        // rdns-discovered placeholders that use the same naming convention.
        let name = hostname.as_deref().unwrap_or(&pod_name);
        let mut pod = Pod::new(name, "");
        if let Ok(ip_addr) = ip.parse::<IpAddr>() {
            pod.system.ips.push(ip_addr);
        }
        let pod_id = pod.entity_id().0.clone();
        facts.new_entities.push(Box::new(pod));

        if !source_id.is_empty() {
            facts
                .new_relations
                .push(Box::new(CanReach::new(source_id, pod_id)));
        }
    }

    ParserOutput::SuccessWithFacts(
        facts,
        format!("discovered {} live host(s) via nmap", hosts.len()),
    )
}

/// Parse nmap greppable (`-oG`) output.
///
/// Returns `(ip, Option<hostname>)` for each live host.
fn parse_nmap_grep(stdout: &str) -> Vec<(String, Option<String>)> {
    let mut hosts = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        // Only process Host: lines.
        let Some(rest) = line.strip_prefix("Host:") else {
            continue;
        };
        let rest = rest.trim();

        // Extract IP (first whitespace-delimited token).
        let Some((ip_str, after_ip)) = rest.split_once(char::is_whitespace) else {
            continue;
        };
        let ip_str = ip_str.trim();
        if ip_str.parse::<IpAddr>().is_err() {
            continue;
        }

        // Hostname is the parenthesised token immediately after the IP.
        let hostname = after_ip
            .trim()
            .strip_prefix('(')
            .and_then(|s| s.split_once(')'))
            .map(|(h, _)| h.trim())
            .filter(|h| !h.is_empty())
            .map(str::to_string);

        // Host is considered live when `Status: Up` or `Ports:` is present.
        let is_up = line.contains("Status: Up") || line.contains("Ports:");
        if is_up {
            hosts.push((ip_str.to_string(), hostname));
        }
    }

    hosts
}

/// Parse nmap XML (`-oX`) output using simple line-by-line string scanning.
///
/// Returns `(ip, Option<hostname>)` for each host with `state="up"`.
fn parse_nmap_xml(stdout: &str) -> Vec<(String, Option<String>)> {
    let mut hosts = Vec::new();

    // Split on `<host` to process one block per host.
    for block in stdout.split("<host") {
        // Check if this host is up — either `<host state="up"` or a child
        // `<status state="up"` element.
        let host_up = block.contains("state=\"up\"");
        if !host_up {
            continue;
        }

        // Extract IPv4 address.
        let ip = extract_xml_attr(block, "address", "addr", Some("addrtype"), Some("ipv4"));
        let Some(ip) = ip else {
            continue;
        };
        if ip.parse::<IpAddr>().is_err() {
            continue;
        }

        // Extract PTR hostname (first `<hostname` with `type="PTR"`).
        let hostname = extract_xml_attr(block, "hostname", "name", Some("type"), Some("PTR"));

        hosts.push((ip, hostname));
    }

    hosts
}

/// Extract an attribute value from a simple XML element.
///
/// Scans `block` for lines containing `<element_name` and optionally a
/// required `guard_attr="guard_val"` pair, then returns the value of
/// `target_attr="..."`.
fn extract_xml_attr(
    block: &str,
    element_name: &str,
    target_attr: &str,
    guard_attr: Option<&str>,
    guard_val: Option<&str>,
) -> Option<String> {
    let tag_prefix = format!("<{}", element_name);
    for line in block.lines() {
        let line = line.trim();
        if !line.contains(&tag_prefix) {
            continue;
        }
        // Check guard attribute if specified.
        if let (Some(attr), Some(val)) = (guard_attr, guard_val) {
            let needle = format!("{}=\"{}\"", attr, val);
            if !line.contains(&needle) {
                continue;
            }
        }
        // Extract target attribute value.
        let attr_needle = format!("{}=\"", target_attr);
        if let Some(start) = line.find(&attr_needle) {
            let val_start = start + attr_needle.len();
            if let Some(end) = line[val_start..].find('"') {
                return Some(line[val_start..val_start + end].to_string());
            }
        }
    }
    None
}

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
fn parse_rdns(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
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
        return ParserOutput::KnownFailure(
            "no valid IP,DNS entries found in rDNS output".to_string(),
        );
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
        let is_pod = dns_parts.first().is_some_and(|&l| l == ip_kebab);
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

    ParserOutput::SuccessWithFacts(facts, format!("discovered {} pod(s) from rDNS", pod_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ran_domain::{CanReach, Entity, Pod};

    // --- nmap ---

    #[test]
    fn parse_nmap_grep_single_status_up_emits_pod_and_can_reach() {
        let stdout = "Host: 10.0.0.5 ()\tStatus: Up\n";
        let result = parse_nmap(stdout, "ns/default/pod/attacker");
        let ParserOutput::SuccessWithFacts(facts, detail) = result else {
            panic!("expected SuccessWithFacts, got {:?}", result);
        };
        assert_eq!(facts.new_entities.len(), 1);
        assert_eq!(facts.new_relations.len(), 1);
        let pod = facts.new_entities[0]
            .as_any()
            .downcast_ref::<Pod>()
            .unwrap();
        assert!(pod.system.ips.iter().any(|ip| ip.to_string() == "10.0.0.5"));
        let rel = facts.new_relations[0]
            .as_any()
            .downcast_ref::<CanReach>()
            .unwrap();
        assert_eq!(rel.source_id.0, "ns/default/pod/attacker");
        assert!(detail.contains("1 live host"));
    }

    #[test]
    fn parse_nmap_grep_multiple_hosts() {
        let stdout = "Host: 10.0.0.5 ()\tStatus: Up\nHost: 10.0.0.6 ()\tStatus: Up\nHost: 10.0.0.7 ()\tStatus: Down\n";
        let ParserOutput::SuccessWithFacts(facts, _) = parse_nmap(stdout, "src") else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 2);
        assert_eq!(facts.new_relations.len(), 2);
    }

    #[test]
    fn parse_nmap_grep_hostname_used_as_pod_name() {
        let stdout = "Host: 10.0.0.6 (redis.default.svc.cluster.local)\tPorts: 6379/open/tcp\n";
        let ParserOutput::SuccessWithFacts(facts, _) = parse_nmap(stdout, "src") else {
            panic!("expected SuccessWithFacts");
        };
        let pod = facts.new_entities[0]
            .as_any()
            .downcast_ref::<Pod>()
            .unwrap();
        assert_eq!(pod.entity_name(), "redis.default.svc.cluster.local");
    }

    #[test]
    fn parse_nmap_xml_host_with_address_emits_pod_and_can_reach() {
        let stdout = r#"<?xml version="1.0"?>
<nmaprun>
<host starttime="1234">
<status state="up" reason="echo-reply"/>
<address addr="10.0.0.5" addrtype="ipv4"/>
</host>
</nmaprun>"#;
        let ParserOutput::SuccessWithFacts(facts, _) = parse_nmap(stdout, "ns/default/pod/scanner")
        else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 1);
        let pod = facts.new_entities[0]
            .as_any()
            .downcast_ref::<Pod>()
            .unwrap();
        assert!(pod.system.ips.iter().any(|ip| ip.to_string() == "10.0.0.5"));
        assert_eq!(facts.new_relations.len(), 1);
        let rel = facts.new_relations[0]
            .as_any()
            .downcast_ref::<CanReach>()
            .unwrap();
        assert_eq!(rel.source_id.0, "ns/default/pod/scanner");
    }

    #[test]
    fn parse_nmap_empty_output_returns_known_failure() {
        assert!(matches!(
            parse_nmap("", "src"),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn parse_nmap_grep_status_down_skipped() {
        let stdout = "Host: 10.0.0.1 ()\tStatus: Down\nHost: 10.0.0.2 ()\tStatus: Down\n";
        assert!(matches!(
            parse_nmap(stdout, "src"),
            ParserOutput::KnownFailure(_)
        ));
    }

    // --- rdns ---

    #[test]
    fn parse_rdns_valid_pod_entries() {
        let stdout = "ip,ptr\n\
            10.244.1.4,10-244-1-4.backend-service.dev.svc.cluster.local\n\
            10.244.1.6,10-244-1-6.argocd-notifications-controller-metrics.argocd.svc.cluster.local\n\
            192.168.0.5,host.local\n";
        let result = parse_rdns(stdout, "", &HashMap::new());
        let ParserOutput::SuccessWithFacts(facts, detail) = result else {
            panic!("expected SuccessWithFacts, got {:?}", result);
        };
        assert_eq!(facts.new_entities.len(), 2, "should discover 2 pods");
        assert!(facts
            .new_entities
            .iter()
            .any(|e| e.entity_kind() == "Pod" && e.entity_name() == "backend-service.10-244-1-4"));
        assert!(facts.new_entities.iter().any(|e| e.entity_kind() == "Pod"
            && e.entity_name() == "argocd-notifications-controller-metrics.10-244-1-6"));
        assert!(detail.contains("2 pod"));
    }

    #[test]
    fn parse_rdns_pod_has_ip_set() {
        let stdout = "10.244.1.4,10-244-1-4.backend-service.dev.svc.cluster.local\n";
        let ParserOutput::SuccessWithFacts(facts, _) = parse_rdns(stdout, "", &HashMap::new())
        else {
            panic!("expected SuccessWithFacts");
        };
        let pod = facts
            .new_entities
            .iter()
            .find(|e| e.entity_kind() == "Pod")
            .unwrap();
        let pod = pod.as_any().downcast_ref::<Pod>().unwrap();
        assert!(pod
            .system
            .ips
            .iter()
            .any(|ip| ip.to_string() == "10.244.1.4"));
        assert_eq!(pod.namespace().unwrap(), "dev");
    }

    #[test]
    fn parse_rdns_skips_non_cluster_local() {
        let stdout = "ip,ptr\n192.168.0.5,host.local\n10.0.0.1,internal.example.com\n";
        let result = parse_rdns(stdout, "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_rdns_empty_input() {
        let result = parse_rdns("", "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_rdns_header_only() {
        let result = parse_rdns("ip,ptr\n", "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_rdns_skips_invalid_lines() {
        let stdout = "ip,ptr\n\
            not-an-ip,some.cluster.local\n\
            this-is-not-csv\n\
            10.0.0.1,10-0-0-1.backend.default.svc.cluster.local\n";
        let result = parse_rdns(stdout, "", &HashMap::new());
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 1);
    }

    #[test]
    fn parse_rdns_without_header() {
        let stdout = "10.244.1.4,10-244-1-4.backend-service.dev.svc.cluster.local\n\
            10.244.1.6,10-244-1-6.argocd-server.argocd.svc.cluster.local\n";
        let result = parse_rdns(stdout, "", &HashMap::new());
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 2);
    }
}
