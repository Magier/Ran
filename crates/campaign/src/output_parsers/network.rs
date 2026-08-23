use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::time::{SystemTime, UNIX_EPOCH};

use ran_domain::{
    AppService, CanReach, Confidence, EndpointState, Entity, HostsService, Pod, Transport,
    UnknownSystem,
};

use super::ParserOutput;
use crate::FactsUpdate;

pub(super) fn register(m: &mut HashMap<&'static str, super::ParserFn>) {
    m.insert("network.discovery", parse_network_discovery);
    // Backward-compat for legacy effect IDs.
    m.insert("rdns", parse_network_discovery);
}

/// Semantic parser for network host discovery.
///
/// Dynamically dispatches based on payload shape:
/// - nmap standard / grep / XML -> nmap parser
/// - `ip,ptr` CSV (reverse DNS output) -> rDNS parser
fn parse_network_discovery(
    stdout: &str,
    stderr: &str,
    args: &HashMap<String, String>,
) -> ParserOutput {
    let data = stdout.trim();
    if data.is_empty() {
        return ParserOutput::KnownFailure("empty network discovery output".to_string());
    }

    // Heuristics for nmap output families.
    let looks_like_nmap = data.starts_with("Starting Nmap")
        || data.contains("Nmap scan report for")
        || data.contains("<nmaprun")
        || data.contains("Host:");
    if looks_like_nmap {
        let source_id = args.get("TARGET_ID").map(String::as_str).unwrap_or("");
        let cidr = args.get("CIDR").map(String::as_str);
        return parse_nmap(data, source_id, cidr);
    }

    // Otherwise treat as reverse-DNS CSV-like output.
    parse_rdns_csv(data, stderr, args)
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
pub(super) fn parse_nmap(stdout: &str, source_id: &str, cidr: Option<&str>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty nmap output".to_string());
    }

    let hosts = if stdout.contains("<nmaprun") || stdout.trim_start().starts_with("<?xml") {
        parse_nmap_xml(stdout)
    } else if stdout.contains("Host:") {
        parse_nmap_grep(stdout)
    } else if stdout.contains("Nmap scan report for") {
        parse_nmap_normal(stdout)
    } else {
        return ParserOutput::UnknownFormat(
            "nmap output is not recognized (expected standard, greppable -oG, or XML -oX format)"
                .to_string(),
        );
    };

    if hosts.is_empty() {
        return ParserOutput::KnownFailure("no live hosts discovered in nmap output".to_string());
    }

    let mut facts = FactsUpdate::default();
    let observed_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok());
    for host in &hosts {
        let ip_addr = host.address;

        // Private discoveries should stay in the scanner's network segment when
        // a CIDR context is available.
        if !is_ip_in_scope(ip_addr, cidr) {
            continue;
        }

        let discovered = classify_discovered_host(ip_addr, host.hostname.as_deref());
        let entity_id = discovered.entity_id().0.clone();
        facts.new_entities.push(discovered);

        if !source_id.is_empty() {
            facts
                .new_relations
                .push(Box::new(CanReach::new(source_id, entity_id.clone())));
        }
        for port in &host.ports {
            let Ok(mut service) = AppService::new(ip_addr.to_string(), port.port, port.transport)
            else {
                continue;
            };
            service.state = port.state;
            service.product = port.product.clone();
            service.version = port.version.clone();
            service.cpes = port.cpes.clone();
            service.banner = port.banner.clone();
            service.confidence = Confidence::Yes;
            service.observed_at_ms = observed_at_ms;
            let service_id = service.entity_id().0.clone();
            facts.new_entities.push(Box::new(service));
            facts.new_relations.push(Box::new(HostsService::new(
                entity_id.clone(),
                service_id.clone(),
            )));
            if !source_id.is_empty() && port.state == EndpointState::Open {
                facts
                    .new_relations
                    .push(Box::new(CanReach::new(source_id, service_id)));
            }
        }
    }

    if facts.new_entities.is_empty() {
        return ParserOutput::KnownFailure(
            "no in-scope live hosts discovered in nmap output".to_string(),
        );
    }

    ParserOutput::SuccessWithFacts(
        facts,
        format!("discovered {} live host(s) via nmap", hosts.len()),
    )
}

#[derive(Debug)]
struct NmapHostObservation {
    address: IpAddr,
    hostname: Option<String>,
    ports: Vec<NmapPortObservation>,
}

#[derive(Debug)]
struct NmapPortObservation {
    port: u16,
    transport: Transport,
    state: EndpointState,
    product: Option<String>,
    version: Option<String>,
    cpes: Vec<String>,
    banner: Option<String>,
}

fn parse_transport(value: &str) -> Transport {
    match value.to_ascii_lowercase().as_str() {
        "tcp" => Transport::Tcp,
        "udp" => Transport::Udp,
        "sctp" => Transport::Sctp,
        _ => Transport::Unknown,
    }
}

fn parse_endpoint_state(value: &str) -> EndpointState {
    match value.to_ascii_lowercase().as_str() {
        "open" => EndpointState::Open,
        "closed" => EndpointState::Closed,
        "filtered" | "open|filtered" | "closed|filtered" => EndpointState::Filtered,
        _ => EndpointState::Unknown,
    }
}

fn normalized_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

fn trimmed_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_cidr_v4(cidr: &str) -> Option<(Ipv4Addr, u8)> {
    let (base, prefix) = cidr.split_once('/')?;
    let base = base.trim().parse::<Ipv4Addr>().ok()?;
    let prefix = prefix.trim().parse::<u8>().ok()?;
    if prefix > 32 {
        return None;
    }
    Some((base, prefix))
}

fn ipv4_in_cidr(ip: Ipv4Addr, base: Ipv4Addr, prefix: u8) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    (u32::from(ip) & mask) == (u32::from(base) & mask)
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_link_local()
}

fn is_ip_in_scope(ip: IpAddr, cidr: Option<&str>) -> bool {
    let IpAddr::V4(ipv4) = ip else {
        return true;
    };

    if !is_private_ipv4(ipv4) {
        return true;
    }

    let Some(cidr) = cidr else {
        return true;
    };
    let Some((base, prefix)) = parse_cidr_v4(cidr) else {
        return true;
    };
    ipv4_in_cidr(ipv4, base, prefix)
}

fn classify_discovered_host(ip: IpAddr, hostname: Option<&str>) -> Box<dyn Entity> {
    let ip_str = ip.to_string();
    let ip_kebab = ip_str.replace('.', "-");

    if let Some(hostname) = hostname {
        if let Some((name, ns)) = derive_cluster_pod_identity(hostname, &ip_kebab) {
            let mut pod = Pod::new(name, ns);
            pod.system.ips.push(ip);
            return Box::new(pod);
        }
    }

    if matches!(ip, IpAddr::V4(v4) if is_private_ipv4(v4)) {
        let name = hostname
            .filter(|h| !h.trim().is_empty())
            .map(|h| h.to_string())
            .unwrap_or_else(|| format!("pod-{}", ip_kebab));
        let mut pod = Pod::new(name, "");
        // The scanner's namespace says where the observation came from, not
        // where the observed IP lives. Keep the placeholder unqualified until
        // DNS or authoritative Kubernetes data supplies a namespace.
        pod.meta.namespace = None;
        pod.system.ips.push(ip);
        return Box::new(pod);
    }

    let mut system = UnknownSystem::new(
        hostname
            .filter(|h| !h.trim().is_empty())
            .unwrap_or(&ip_str)
            .to_string(),
    );
    system.system.ips.push(ip);
    Box::new(system)
}

fn derive_cluster_pod_identity(hostname: &str, ip_kebab: &str) -> Option<(String, String)> {
    if !hostname.ends_with("cluster.local") {
        return None;
    }
    let parts: Vec<&str> = hostname.split('.').collect();
    let first = parts.first().copied().unwrap_or("");
    if first != ip_kebab {
        return None;
    }

    let (name, ns) = match parts.len() {
        4 => (parts[0].to_string(), parts[0].to_string()),
        5 => (parts[0].to_string(), parts[1].to_string()),
        6 => (format!("{}.{}", parts[1], parts[0]), parts[2].to_string()),
        n if n > 6 => (parts[0].to_string(), parts[2].to_string()),
        _ => return None,
    };
    if ns.is_empty() || name.is_empty() {
        return None;
    }
    Some((name, ns))
}

/// Parse nmap greppable (`-oG`) output.
///
/// Returns a structured observation for each live host.
fn parse_nmap_grep(stdout: &str) -> Vec<NmapHostObservation> {
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
            let ports = line
                .split_once("Ports:")
                .map(|(_, value)| value.split("Ignored State:").next().unwrap_or(value))
                .into_iter()
                .flat_map(|value| value.split(','))
                .filter_map(|entry| {
                    let cols: Vec<&str> = entry.trim().split('/').collect();
                    let port = cols
                        .first()?
                        .trim()
                        .parse::<u16>()
                        .ok()
                        .filter(|p| *p != 0)?;
                    let state = parse_endpoint_state(cols.get(1).copied().unwrap_or(""));
                    let transport = parse_transport(cols.get(2).copied().unwrap_or(""));
                    let service = normalized_text(cols.get(4).copied().unwrap_or(""));
                    let version = cols.get(6).and_then(|v| trimmed_text(v));
                    Some(NmapPortObservation {
                        port,
                        transport,
                        state,
                        product: service,
                        version,
                        cpes: Vec::new(),
                        banner: None,
                    })
                })
                .collect();
            hosts.push(NmapHostObservation {
                address: ip_str.parse().unwrap(),
                hostname,
                ports,
            });
        }
    }

    hosts
}

/// Parse standard nmap text output (`nmap ...`).
///
/// Recognized host lines:
/// - `Nmap scan report for 10.0.0.5`
/// - `Nmap scan report for redis.default.svc.cluster.local (10.0.0.6)`
///
/// Returns structured host and port observations.
fn parse_nmap_normal(stdout: &str) -> Vec<NmapHostObservation> {
    let mut hosts = Vec::new();
    let mut current: Option<NmapHostObservation> = None;
    let mut in_ports = false;
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Nmap scan report for ") {
            if let Some(host) = current.take() {
                hosts.push(host);
            }
            let (address, hostname) = if let Some((host, tail)) = rest.rsplit_once(" (") {
                (
                    tail.strip_suffix(')').and_then(|v| v.trim().parse().ok()),
                    Some(host.trim().to_string()),
                )
            } else {
                (rest.trim().parse().ok(), None)
            };
            current = address.map(|address| NmapHostObservation {
                address,
                hostname,
                ports: Vec::new(),
            });
            in_ports = false;
            continue;
        }
        if line.starts_with("PORT") && line.contains("STATE") && line.contains("SERVICE") {
            in_ports = true;
            continue;
        }
        if in_ports
            && (line.is_empty()
                || line.starts_with("Service detection")
                || line.starts_with("MAC Address")
                || line.starts_with("Nmap done"))
        {
            in_ports = false;
        }
        if in_ports {
            let mut cols = line.split_whitespace();
            let Some(port_proto) = cols.next() else {
                continue;
            };
            let Some((port, proto)) = port_proto.split_once('/') else {
                continue;
            };
            let Some(port) = port.parse::<u16>().ok().filter(|p| *p != 0) else {
                continue;
            };
            let state = parse_endpoint_state(cols.next().unwrap_or(""));
            let product = normalized_text(cols.next().unwrap_or(""));
            let version = trimmed_text(&cols.collect::<Vec<_>>().join(" "));
            if let Some(host) = current.as_mut() {
                host.ports.push(NmapPortObservation {
                    port,
                    transport: parse_transport(proto),
                    state,
                    product,
                    version,
                    cpes: Vec::new(),
                    banner: None,
                });
            }
        }
    }
    if let Some(host) = current {
        hosts.push(host);
    }

    hosts
}

/// Parse nmap XML (`-oX`) output using simple line-by-line string scanning.
///
/// Returns structured observations for each host with `state="up"`.
fn parse_nmap_xml(stdout: &str) -> Vec<NmapHostObservation> {
    let mut hosts = Vec::new();

    // Split on `<host` to process one block per host.
    for block in stdout.split("<host") {
        // Check if this host is up — either `<host state="up"` or a child
        // `<status state="up"` element.
        let host_up = element_tags(block, "status")
            .any(|tag| extract_attr(tag, "state").as_deref() == Some("up"));
        if !host_up {
            continue;
        }

        let ip = element_tags(block, "address").find_map(|tag| {
            let kind = extract_attr(tag, "addrtype")?;
            matches!(kind.as_str(), "ipv4" | "ipv6")
                .then(|| extract_attr(tag, "addr"))
                .flatten()
        });
        let Some(ip) = ip else {
            continue;
        };
        if ip.parse::<IpAddr>().is_err() {
            continue;
        }

        // Extract PTR hostname (first `<hostname` with `type="PTR"`).
        let hostname = extract_xml_attr(block, "hostname", "name", Some("type"), Some("PTR"));

        let ports = block
            .split("<port ")
            .skip(1)
            .filter_map(|port_block| {
                let tag = port_block.split_once('>')?.0;
                let port = extract_attr(tag, "portid")?
                    .parse::<u16>()
                    .ok()
                    .filter(|p| *p != 0)?;
                let transport = parse_transport(&extract_attr(tag, "protocol").unwrap_or_default());
                let state = port_block
                    .find("<state ")
                    .and_then(|start| port_block[start..].split_once('>').map(|v| v.0))
                    .and_then(|tag| extract_attr(tag, "state"))
                    .map(|v| parse_endpoint_state(&v))
                    .unwrap_or(EndpointState::Unknown);
                let service_tag = port_block
                    .find("<service ")
                    .and_then(|start| port_block[start..].split_once('>').map(|v| v.0));
                let product = service_tag
                    .and_then(|tag| {
                        extract_attr(tag, "product").or_else(|| extract_attr(tag, "name"))
                    })
                    .and_then(|v| normalized_text(&v));
                let version = service_tag
                    .and_then(|tag| extract_attr(tag, "version"))
                    .and_then(|v| trimmed_text(&v));
                let extrainfo = service_tag.and_then(|tag| extract_attr(tag, "extrainfo"));
                let cpes = port_block
                    .split("<cpe>")
                    .skip(1)
                    .filter_map(|v| v.split_once("</cpe>").map(|x| x.0.trim().to_string()))
                    .collect();
                Some(NmapPortObservation {
                    port,
                    transport,
                    state,
                    product,
                    version,
                    cpes,
                    banner: extrainfo,
                })
            })
            .collect();
        hosts.push(NmapHostObservation {
            address: ip.parse().unwrap(),
            hostname,
            ports,
        });
    }

    hosts
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')?;
    Some(tag[start..start + end].to_string())
}

fn element_tags<'a>(block: &'a str, element: &str) -> std::vec::IntoIter<&'a str> {
    let prefix = format!("<{element}");
    block
        .match_indices(&prefix)
        .filter_map(move |(start, _)| block[start..].split_once('>').map(|v| v.0))
        .collect::<Vec<_>>()
        .into_iter()
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
fn parse_rdns_csv(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
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
    use ran_domain::{AppService, CanReach, Entity, HostsService, Pod};

    // --- nmap ---

    #[test]
    fn parse_nmap_grep_single_status_up_emits_pod_and_can_reach() {
        let stdout = "Host: 10.0.0.5 ()\tStatus: Up\n";
        let result = parse_nmap(stdout, "ns/default/pod/attacker", None);
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
        let ParserOutput::SuccessWithFacts(facts, _) = parse_nmap(stdout, "src", None) else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 2);
        assert_eq!(facts.new_relations.len(), 2);
    }

    #[test]
    fn parse_nmap_grep_hostname_used_as_pod_name() {
        let stdout = "Host: 10.0.0.6 (redis.default.svc.cluster.local)\tPorts: 6379/open/tcp\n";
        let ParserOutput::SuccessWithFacts(facts, _) = parse_nmap(stdout, "src", None) else {
            panic!("expected SuccessWithFacts");
        };
        let pod = facts.new_entities[0]
            .as_any()
            .downcast_ref::<Pod>()
            .unwrap();
        assert_eq!(pod.entity_name(), "redis.default.svc.cluster.local");
        let service = facts
            .new_entities
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<AppService>())
            .unwrap();
        assert_eq!(service.port, 6379);
        assert_eq!(service.state, EndpointState::Open);
        assert!(facts
            .new_relations
            .iter()
            .any(|r| r.as_any().is::<HostsService>()));
        assert!(facts.new_relations.iter().any(|r| {
            r.as_any()
                .downcast_ref::<CanReach>()
                .is_some_and(|reach| reach.target_id == service.entity_id())
        }));
    }

    #[test]
    fn parse_nmap_standard_service_version() {
        let stdout = "Nmap scan report for 10.0.0.8\nHost is up\nPORT     STATE SERVICE VERSION\n6379/tcp open  redis   Redis key-value store 7.2.4\n";
        let ParserOutput::SuccessWithFacts(facts, _) = parse_nmap(stdout, "src", None) else {
            panic!()
        };
        let service = facts
            .new_entities
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<AppService>())
            .unwrap();
        assert_eq!(service.product.as_deref(), Some("redis"));
        assert_eq!(
            service.version.as_deref(),
            Some("Redis key-value store 7.2.4")
        );
    }

    #[test]
    fn parse_nmap_xml_service_metadata_and_closed_connectivity() {
        let stdout = r#"<nmaprun><host><status state="up"/><address addr="10.0.0.9" addrtype="ipv4"/><ports>
<port protocol="tcp" portid="6379"><state state="open"/><service name="redis" product="Redis" version="6.0" extrainfo="test"><cpe>cpe:/a:redis:redis:6.0</cpe></service></port>
<port protocol="tcp" portid="6380"><state state="closed"/></port></ports></host></nmaprun>"#;
        let ParserOutput::SuccessWithFacts(facts, _) = parse_nmap(stdout, "src", None) else {
            panic!()
        };
        let services: Vec<_> = facts
            .new_entities
            .iter()
            .filter_map(|e| e.as_any().downcast_ref::<AppService>())
            .collect();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].product.as_deref(), Some("redis"));
        assert_eq!(services[0].cpes, ["cpe:/a:redis:redis:6.0"]);
        assert_eq!(
            facts
                .new_relations
                .iter()
                .filter(|r| {
                    r.relation_name() == "can-reach" && r.target_id().0.starts_with("app-service/")
                })
                .count(),
            1
        );
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
        let ParserOutput::SuccessWithFacts(facts, _) =
            parse_nmap(stdout, "ns/default/pod/scanner", None)
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
            parse_nmap("", "src", None),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn parse_nmap_standard_output_discovers_hosts() {
        let stdout = "Starting Nmap 7.95\nNmap scan report for 10-0-0-13.oopservability-agent.oopservability.svc.cluster.local (10.0.0.13)\nHost is up\nNmap scan report for 10.0.0.44\nHost is up\n";
        let ParserOutput::SuccessWithFacts(facts, _) = parse_nmap(stdout, "src", None) else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 2);
    }

    #[test]
    fn parse_nmap_grep_status_down_skipped() {
        let stdout = "Host: 10.0.0.1 ()\tStatus: Down\nHost: 10.0.0.2 ()\tStatus: Down\n";
        assert!(matches!(
            parse_nmap(stdout, "src", None),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn parse_nmap_private_hosts_filtered_by_cidr_scope() {
        let stdout = "Host: 10.0.0.5 ()\tStatus: Up\nHost: 10.0.1.6 ()\tStatus: Up\n";
        let ParserOutput::SuccessWithFacts(facts, _) =
            parse_nmap(stdout, "ns/dungeon/pod/scanner", Some("10.0.0.0/24"))
        else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 1);
        let pod = facts.new_entities[0]
            .as_any()
            .downcast_ref::<Pod>()
            .unwrap();
        assert_eq!(pod.namespace(), None);
        assert!(pod.system.ips.iter().any(|ip| ip.to_string() == "10.0.0.5"));
    }

    #[test]
    fn parse_nmap_ip_only_host_does_not_inherit_scanner_namespace() {
        let stdout = "Nmap scan report for 10.0.0.137\nHost is up (0.00080s latency).\nAll 100 scanned ports on 10.0.0.137 are in ignored states.\n";
        let ParserOutput::SuccessWithFacts(facts, _) =
            parse_nmap(stdout, "ns/dungeon/pod/scanner", None)
        else {
            panic!("expected SuccessWithFacts");
        };
        let pod = facts.new_entities[0]
            .as_any()
            .downcast_ref::<Pod>()
            .unwrap();
        assert_eq!(pod.namespace(), None);
        assert_eq!(pod.entity_name(), "pod-10-0-0-137");
    }

    #[test]
    fn parse_nmap_cluster_local_hostname_derives_namespace() {
        let stdout = "Nmap scan report for 10-0-0-13.oopservability-agent.oopservability.svc.cluster.local (10.0.0.13)\nHost is up\n";
        let ParserOutput::SuccessWithFacts(facts, _) =
            parse_nmap(stdout, "ns/dungeon/pod/scanner", None)
        else {
            panic!("expected SuccessWithFacts");
        };
        let pod = facts.new_entities[0]
            .as_any()
            .downcast_ref::<Pod>()
            .unwrap();
        assert_eq!(pod.namespace(), Some("oopservability"));
    }

    // --- rdns ---

    #[test]
    fn parse_rdns_valid_pod_entries() {
        let stdout = "ip,ptr\n\
            10.244.1.4,10-244-1-4.backend-service.dev.svc.cluster.local\n\
            10.244.1.6,10-244-1-6.argocd-notifications-controller-metrics.argocd.svc.cluster.local\n\
            192.168.0.5,host.local\n";
        let result = parse_network_discovery(stdout, "", &HashMap::new());
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
        let ParserOutput::SuccessWithFacts(facts, _) =
            parse_network_discovery(stdout, "", &HashMap::new())
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
        let result = parse_network_discovery(stdout, "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_rdns_empty_input() {
        let result = parse_network_discovery("", "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_rdns_header_only() {
        let result = parse_network_discovery("ip,ptr\n", "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_rdns_skips_invalid_lines() {
        let stdout = "ip,ptr\n\
            not-an-ip,some.cluster.local\n\
            this-is-not-csv\n\
            10.0.0.1,10-0-0-1.backend.default.svc.cluster.local\n";
        let result = parse_network_discovery(stdout, "", &HashMap::new());
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 1);
    }

    #[test]
    fn parse_rdns_without_header() {
        let stdout = "10.244.1.4,10-244-1-4.backend-service.dev.svc.cluster.local\n\
            10.244.1.6,10-244-1-6.argocd-server.argocd.svc.cluster.local\n";
        let result = parse_network_discovery(stdout, "", &HashMap::new());
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 2);
    }
}
