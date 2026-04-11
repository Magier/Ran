use std::collections::HashMap;
use std::net::IpAddr;

use ran_domain::Pod;

use crate::FactsUpdate;
use super::ParserOutput;

pub(super) fn register(m: &mut HashMap<&'static str, super::ParserFn>) {
    m.insert("rdns", parse_rdns);
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

#[cfg(test)]
mod tests {
    use super::*;
    use ran_domain::Pod;

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
