//! Reconstruct Kubernetes Service facts from container environment variables.
//!
//! Kubelet injects a set of environment variables into every container it
//! starts (`getServiceEnvVarMap` in `pkg/kubelet/kubelet_pods.go`, rendered by
//! `pkg/kubelet/envvars`). Two tiers exist:
//!
//! * The `kubernetes` Service from the `default` namespace is **always**
//!   injected - `KUBERNETES_SERVICE_HOST`, `KUBERNETES_SERVICE_PORT` and the
//!   Docker-legacy `KUBERNETES_PORT_443_TCP*` family - regardless of the pod's
//!   `enableServiceLinks` setting.
//! * Every other Service is injected only when it lives in the pod's own
//!   namespace, carries a real ClusterIP (headless services are skipped),
//!   existed before the container started, and `enableServiceLinks` is true
//!   (the PodSpec default).
//!
//! Consequences for inference:
//!
//! * `KUBERNETES_SERVICE_HOST` is a reliable "kubelet injected this" signal,
//!   and is not distro-specific - only its value varies between clusters.
//! * The *absence* of `<NAME>_SERVICE_HOST` entries proves nothing: charts
//!   commonly set `enableServiceLinks: false`, headless services never appear,
//!   and cross-namespace services are never linked.
//! * Every value is a snapshot from container-start time, so a Service that has
//!   since been deleted or re-assigned a ClusterIP still shows its old address.
//! * The values are writable by the workload, so nothing derived here is
//!   authoritative - entities keep the default `NameConfidence::Derived`.

use std::collections::HashMap;
use std::net::IpAddr;

use ran_domain::K8sServicePort;

/// Namespace of the Service whose address kubelet always injects.
pub const MASTER_SERVICE_NAMESPACE: &str = "default";

/// Name of that Service.
pub const MASTER_SERVICE_NAME: &str = "kubernetes";

const SERVICE_HOST_SUFFIX: &str = "_SERVICE_HOST";
const SERVICE_PORT_SUFFIX: &str = "_SERVICE_PORT";

/// A Kubernetes Service reconstructed from environment variables.
#[derive(Debug, Clone)]
pub struct EnvService {
    /// Service name, mapped back from the env-var prefix.
    pub name: String,
    /// ClusterIP, read from `<PREFIX>_SERVICE_HOST`.
    pub cluster_ip: String,
    /// Ports, read from `<PREFIX>_SERVICE_PORT[_<PORTNAME>]`, ordered by number.
    pub ports: Vec<K8sServicePort>,
}

impl EnvService {
    /// True for the `kubernetes` Service, which kubelet injects into every
    /// container regardless of `enableServiceLinks`. It lives in the `default`
    /// namespace, not in the namespace of the pod that observed it.
    pub fn is_master_service(&self) -> bool {
        self.name == MASTER_SERVICE_NAME
    }
}

/// Extract every Service described by the given environment variables.
///
/// The heuristic follows kubelet's own generator in reverse:
///
/// 1. Every `<PREFIX>_SERVICE_HOST` key names one Service. A Service with no
///    host entry cannot be located, so it is skipped.
/// 2. The host value must parse as an IP address. ClusterIPs always do, and the
///    check rejects a Service whose *port* happens to be named `service-host`
///    (which would otherwise produce the key `FOO_SERVICE_PORT_SERVICE_HOST`
///    and be mistaken for a Service called `foo-service-port`).
/// 3. Ports come from the exact key `<PREFIX>_SERVICE_PORT` and from keys
///    prefixed `<PREFIX>_SERVICE_PORT_`. Matching the full prefix rather than
///    just `<PREFIX>` keeps sibling Services apart: `redis` must not absorb the
///    variables belonging to `redis-replica`.
///
/// Results are sorted by name so callers see a deterministic order.
pub fn services_from_env(env: &HashMap<String, String>) -> Vec<EnvService> {
    let mut prefixes: Vec<&str> = env
        .keys()
        .filter_map(|key| key.strip_suffix(SERVICE_HOST_SUFFIX))
        .filter(|prefix| !prefix.is_empty())
        .collect();
    prefixes.sort_unstable();

    let mut services = Vec::new();
    for prefix in prefixes {
        let Some(host) = env.get(&format!("{prefix}{SERVICE_HOST_SUFFIX}")) else {
            continue;
        };
        let host = host.trim();
        if host.parse::<IpAddr>().is_err() {
            continue;
        }

        services.push(EnvService {
            name: service_name_from_env(prefix),
            cluster_ip: host.to_string(),
            ports: ports_for_prefix(env, prefix),
        });
    }

    services.sort_by(|a, b| a.name.cmp(&b.name));
    services
}

/// Map an env-var fragment back to the Kubernetes object name it came from.
///
/// Kubelet upper-cases the name and rewrites `-` as `_`. The mapping is
/// lossless in reverse because Service names are DNS-1035 labels and port names
/// are IANA service names - neither may contain an underscore or an upper-case
/// character.
fn service_name_from_env(fragment: &str) -> String {
    fragment.to_ascii_lowercase().replace('_', "-")
}

fn ports_for_prefix(env: &HashMap<String, String>, prefix: &str) -> Vec<K8sServicePort> {
    let named_prefix = format!("{prefix}{SERVICE_PORT_SUFFIX}_");
    let mut ports: Vec<K8sServicePort> = Vec::new();

    for (key, value) in env {
        let Some(port_name) = key.strip_prefix(&named_prefix) else {
            continue;
        };
        if port_name.is_empty() {
            continue;
        }
        let Ok(port) = value.trim().parse::<i32>() else {
            continue;
        };
        ports.push(K8sServicePort {
            port,
            target_port: String::new(),
            protocol: protocol_for_port(env, prefix, port),
            name: Some(service_name_from_env(port_name)),
            node_port: None,
        });
    }

    // The unnamed `<PREFIX>_SERVICE_PORT` repeats the Service's first port, so
    // it only adds information when no named entry already covers that number.
    if let Some(value) = env.get(&format!("{prefix}{SERVICE_PORT_SUFFIX}")) {
        if let Ok(port) = value.trim().parse::<i32>() {
            if !ports.iter().any(|existing| existing.port == port) {
                ports.push(K8sServicePort {
                    port,
                    target_port: String::new(),
                    protocol: protocol_for_port(env, prefix, port),
                    name: None,
                    node_port: None,
                });
            }
        }
    }

    ports.sort_by(|a, b| a.port.cmp(&b.port).then_with(|| a.name.cmp(&b.name)));
    ports
}

/// Recover a port's protocol from the Docker-legacy variables kubelet also
/// emits (`<PREFIX>_PORT_<port>_<PROTO>`).
///
/// Those variables are keyed by port *number*, while named ports are keyed by
/// name, so the two cannot be joined when one number is exposed under several
/// protocols - CoreDNS publishes `dns` (UDP) and `dns-tcp` (TCP) both on 53.
/// A protocol is therefore only reported when it is the sole one advertised for
/// that number; otherwise the Kubernetes default of TCP stands.
fn protocol_for_port(env: &HashMap<String, String>, prefix: &str, port: i32) -> String {
    let advertised: Vec<&str> = ["TCP", "UDP", "SCTP"]
        .into_iter()
        .filter(|proto| env.contains_key(&format!("{prefix}_PORT_{port}_{proto}")))
        .collect();

    match advertised.as_slice() {
        [only] => only.to_string(),
        _ => "TCP".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// The variable block every kubelet-managed container receives, taken from
    /// the Kubernetes container-environment documentation.
    fn master_service_env() -> HashMap<String, String> {
        env(&[
            ("KUBERNETES_PORT", "tcp://10.96.0.1:443"),
            ("KUBERNETES_PORT_443_TCP", "tcp://10.96.0.1:443"),
            ("KUBERNETES_PORT_443_TCP_ADDR", "10.96.0.1"),
            ("KUBERNETES_PORT_443_TCP_PORT", "443"),
            ("KUBERNETES_PORT_443_TCP_PROTO", "tcp"),
            ("KUBERNETES_SERVICE_HOST", "10.96.0.1"),
            ("KUBERNETES_SERVICE_PORT", "443"),
            ("KUBERNETES_SERVICE_PORT_HTTPS", "443"),
        ])
    }

    #[test]
    fn extracts_the_master_service() {
        let services = services_from_env(&master_service_env());

        assert_eq!(services.len(), 1);
        let svc = &services[0];
        assert_eq!(svc.name, "kubernetes");
        assert!(svc.is_master_service());
        assert_eq!(svc.cluster_ip, "10.96.0.1");
        // `KUBERNETES_SERVICE_PORT` and `KUBERNETES_SERVICE_PORT_HTTPS` are the
        // same port - the named entry wins and the number is not duplicated.
        assert_eq!(svc.ports.len(), 1);
        assert_eq!(svc.ports[0].port, 443);
        assert_eq!(svc.ports[0].name.as_deref(), Some("https"));
        assert_eq!(svc.ports[0].protocol, "TCP");
    }

    #[test]
    fn maps_underscores_back_to_dashes_in_service_names() {
        let services = services_from_env(&env(&[
            ("MY_SVC_SERVICE_HOST", "10.0.0.7"),
            ("MY_SVC_SERVICE_PORT", "8080"),
        ]));

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "my-svc");
        assert_eq!(services[0].ports[0].port, 8080);
        assert_eq!(services[0].ports[0].name, None);
    }

    #[test]
    fn sibling_services_do_not_absorb_each_others_ports() {
        // The legacy Go implementation matched variables by `HasPrefix(key, name)`,
        // so `REDIS` also swept up every `REDIS_REPLICA_*` entry.
        let services = services_from_env(&env(&[
            ("REDIS_SERVICE_HOST", "10.0.0.10"),
            ("REDIS_SERVICE_PORT", "6379"),
            ("REDIS_REPLICA_SERVICE_HOST", "10.0.0.11"),
            ("REDIS_REPLICA_SERVICE_PORT", "6380"),
            ("REDIS_REPLICA_SERVICE_PORT_METRICS", "9121"),
        ]));

        assert_eq!(services.len(), 2);

        let redis = services.iter().find(|s| s.name == "redis").unwrap();
        assert_eq!(redis.cluster_ip, "10.0.0.10");
        assert_eq!(redis.ports.len(), 1);
        assert_eq!(redis.ports[0].port, 6379);

        let replica = services.iter().find(|s| s.name == "redis-replica").unwrap();
        assert_eq!(replica.cluster_ip, "10.0.0.11");
        assert_eq!(
            replica.ports.iter().map(|p| p.port).collect::<Vec<_>>(),
            vec![6380, 9121]
        );
    }

    #[test]
    fn skips_services_without_a_host_entry() {
        // Ports alone cannot locate a service.
        let services = services_from_env(&env(&[("ORPHAN_SERVICE_PORT", "8080")]));
        assert!(services.is_empty());
    }

    #[test]
    fn skips_host_values_that_are_not_ip_addresses() {
        // A port named `service-host` yields `FOO_SERVICE_PORT_SERVICE_HOST`,
        // which ends in the host suffix but holds a port number.
        let services = services_from_env(&env(&[
            ("FOO_SERVICE_HOST", "10.0.0.5"),
            ("FOO_SERVICE_PORT", "80"),
            ("FOO_SERVICE_PORT_SERVICE_HOST", "8080"),
        ]));

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "foo");
    }

    #[test]
    fn handles_a_service_whose_name_ends_in_the_suffix() {
        let services = services_from_env(&env(&[
            ("FOO_SERVICE_HOST_SERVICE_HOST", "10.0.0.9"),
            ("FOO_SERVICE_HOST_SERVICE_PORT", "80"),
        ]));

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "foo-service-host");
        assert_eq!(services[0].cluster_ip, "10.0.0.9");
    }

    #[test]
    fn reads_protocol_from_docker_legacy_variables() {
        let services = services_from_env(&env(&[
            ("SYSLOG_SERVICE_HOST", "10.0.0.20"),
            ("SYSLOG_SERVICE_PORT", "514"),
            ("SYSLOG_PORT_514_UDP", "udp://10.0.0.20:514"),
        ]));

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].ports[0].protocol, "UDP");
    }

    #[test]
    fn falls_back_to_tcp_when_one_port_number_serves_several_protocols() {
        // CoreDNS publishes `dns` on UDP 53 and `dns-tcp` on TCP 53. The legacy
        // variables are keyed by number only, so neither named port can claim a
        // protocol and both keep the Kubernetes default.
        let services = services_from_env(&env(&[
            ("KUBE_DNS_SERVICE_HOST", "10.96.0.10"),
            ("KUBE_DNS_SERVICE_PORT_DNS", "53"),
            ("KUBE_DNS_SERVICE_PORT_DNS_TCP", "53"),
            ("KUBE_DNS_PORT_53_UDP", "udp://10.96.0.10:53"),
            ("KUBE_DNS_PORT_53_TCP", "tcp://10.96.0.10:53"),
        ]));

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "kube-dns");
        assert_eq!(
            services[0]
                .ports
                .iter()
                .map(|p| (p.name.as_deref().unwrap_or(""), p.protocol.as_str()))
                .collect::<Vec<_>>(),
            vec![("dns", "TCP"), ("dns-tcp", "TCP")]
        );
    }

    #[test]
    fn ignores_unrelated_environment_variables() {
        let services = services_from_env(&env(&[
            ("HOME", "/root"),
            ("PATH", "/usr/bin"),
            ("DATABASE_URL", "postgres://user:pw@db:5432/app"),
        ]));
        assert!(services.is_empty());
    }

    #[test]
    fn ignores_unparseable_port_values() {
        let services = services_from_env(&env(&[
            ("APP_SERVICE_HOST", "10.0.0.3"),
            ("APP_SERVICE_PORT", "not-a-port"),
        ]));

        assert_eq!(services.len(), 1);
        assert!(services[0].ports.is_empty());
    }
}
