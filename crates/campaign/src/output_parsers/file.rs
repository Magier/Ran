use super::ParserOutput;
use crate::FactsUpdate;
use ran_domain::{AuthenticatesTo, Contains, Entity, K8sCluster, K8sCredential, Namespace, Uses};

// ---------------------------------------------------------------------------
// Path extraction
// ---------------------------------------------------------------------------

/// Extract the path argument from a parametric `file:content(...)` effect ID.
///
/// `file:content(/etc/kubernetes/admin.conf)` → `/etc/kubernetes/admin.conf`
/// `file:content(/var/run/secrets/token)` → `/var/run/secrets/token`
pub(super) fn extract_path(effect_id: &str) -> Option<&str> {
    // Find the first '(' and the last ')' to support paths with nested parens.
    let open = effect_id.find('(')?;
    let close = effect_id.rfind(')')?;
    if close <= open {
        return None;
    }
    Some(effect_id[open + 1..close].trim())
}

// ---------------------------------------------------------------------------
// Kubeconfig heuristic
// ---------------------------------------------------------------------------

/// Returns `true` when `content` looks like a kubeconfig YAML.
///
/// Checks for the three mandatory kubeconfig markers:
/// - `apiVersion: v1`
/// - `kind: Config`
/// - `clusters:`
///
/// This is intentionally broad: we do not parse the YAML here, just scan for
/// the literal strings, which is enough to distinguish kubeconfig from generic
/// YAML or plaintext files.
pub(super) fn is_kubeconfig_content(content: &str) -> bool {
    content.contains("apiVersion: v1")
        && content.contains("kind: Config")
        && content.contains("clusters:")
}

// ---------------------------------------------------------------------------
// Kubeconfig YAML parsing
// ---------------------------------------------------------------------------

/// Parse kubeconfig YAML and build a `K8sCredential` entity.
///
/// Extracts the first cluster's `server` and `certificate-authority-data`, and
/// the first user's `token` or `client-certificate-data` + `client-key-data`.
///
/// Returns `None` when the YAML does not contain a usable cluster entry.
fn credential_from_kubeconfig(content: &str) -> Option<(K8sCredential, String)> {
    let resolved = k8s::resolve_kubeconfig_yaml(content, None).ok()?;
    let cluster_name = resolved.cluster_name.clone();
    let mut cred = K8sCredential::new(resolved.server.clone().unwrap_or_default());
    cred.context_name = Some(resolved.context_name);
    cred.default_namespace = resolved.default_namespace;
    cred.user_name = resolved.user_name;
    cred.auth_method = resolved.auth_method;
    cred.has_token = resolved.has_token;
    cred.has_client_certificate = resolved.has_client_certificate;
    cred.has_client_key = resolved.has_client_key;
    cred.ca_data = resolved.ca_data;
    cred.token = resolved.token;
    cred.cert_data = resolved.cert_data;
    cred.key_data = resolved.key_data;

    Some((cred, cluster_name))
}

// ---------------------------------------------------------------------------
// Public parser entry points (called from parse_output_effect with source_id)
// ---------------------------------------------------------------------------

/// Parse kubeconfig YAML and emit a `K8sCredential` entity plus a `Uses` relation
/// from `source_id` → credential.
///
/// Called for both `file:kubeconfig` (explicit) and the kubeconfig branch of
/// `file:content(...)`.
///
/// Returns:
/// - `SuccessWithFacts` — credential entity (and optional Uses relation) emitted
/// - `KnownFailure` — empty content
/// - `UnknownFormat` — non-empty content that fails YAML parsing or has no cluster entry
pub(super) fn parse_file_kubeconfig(stdout: &str, source_id: &str) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout for file:kubeconfig".to_string());
    }

    let (cred, cluster_name) = match credential_from_kubeconfig(stdout) {
        Some(c) => c,
        None => {
            return ParserOutput::UnknownFormat(
                "could not extract cluster/user from kubeconfig YAML".to_string(),
            )
        }
    };

    let detail = format!(
        "extracted K8sCredential for endpoint '{}' (context={}, cluster={}, default_namespace={}, token={}, cert={})",
        if cred.endpoint.is_empty() {
            "unknown"
        } else {
            &cred.endpoint
        },
        cred.context_name.as_deref().unwrap_or("unknown"),
        cluster_name,
        cred.default_namespace.as_deref().unwrap_or("none"),
        cred.token.is_some(),
        cred.cert_data.is_some(),
    );

    let cred_id = cred.entity_id().0.clone();
    let mut facts = FactsUpdate::default();
    facts.new_entities.push(Box::new(cred));
    if !source_id.is_empty() {
        facts
            .new_relations
            .push(Box::new(Uses::new(source_id, cred_id)));
    }

    ParserOutput::SuccessWithFacts(facts, detail)
}

/// Parse the kubeconfig read from the machine running Ran and emit the
/// **active** Kubernetes identity plus its cluster.
///
/// Unlike [`parse_file_kubeconfig`] (which records a knowledge-only credential
/// discovered on some remote system), this reproduces the graph shape Ran used
/// to seed at bootstrap for its own kubeconfig:
/// - a `K8sCredential` with `active = true`
/// - the `K8sCluster` it authenticates to
/// - `AuthenticatesTo(credential → cluster)`
/// - `Contains(source_id → credential)` where `source_id` is the operator host
/// - when the context declares a default namespace, the `Namespace` entity and
///   `Contains(cluster → namespace)`
///
/// `source_id` is the operator-host entity (`system/operator-host`). When empty
/// the containment relation is skipped.
///
/// TODO(tech-debt): this duplicates most of [`parse_file_kubeconfig`]. Kubeconfig
/// parsing is format-invariant — the API server and user identity are inferred
/// the same way regardless of origin. The only real distinction (local/active
/// vs in-cluster discovery) is a *provenance* concern and should drive `active`
/// and cluster-graph emission from a single parser, rather than being encoded as
/// a separate effect + function. See memory
/// `project_kubeconfig_effect_provenance_debt`.
///
/// Returns:
/// - `SuccessWithFacts` — active credential, cluster, and relations emitted
/// - `KnownFailure` — empty content
/// - `UnknownFormat` — non-empty content that fails to resolve to a cluster/user
pub(super) fn parse_local_kubeconfig(stdout: &str, source_id: &str) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout for file:local-kubeconfig".to_string());
    }

    let (mut cred, cluster_name) = match credential_from_kubeconfig(stdout) {
        Some(c) => c,
        None => {
            return ParserOutput::UnknownFormat(
                "could not extract cluster/user from local kubeconfig YAML".to_string(),
            )
        }
    };
    cred.active = true;
    // A raw server URL is an unfriendly display name / id. Prefer the context
    // name (what kubectl shows), then the user name, then fall back to the
    // endpoint that `K8sCredential::new` defaulted to.
    if let Some(label) = cred
        .context_name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            cred.user_name
                .clone()
                .filter(|value| !value.trim().is_empty())
        })
    {
        cred.name = label;
    }

    let mut cluster = K8sCluster::new(&cluster_name);
    cluster.context_name = cred.context_name.clone();
    if !cred.endpoint.is_empty() {
        cluster.server = Some(cred.endpoint.clone());
    }
    let cluster_id = cluster.entity_id().0.clone();
    let cred_id = cred.entity_id().0.clone();
    let default_namespace = cred.default_namespace.clone();

    let detail = format!(
        "established active K8sCredential for endpoint '{}' (context={}, cluster={}, default_namespace={}, token={}, cert={})",
        if cred.endpoint.is_empty() {
            "unknown"
        } else {
            &cred.endpoint
        },
        cred.context_name.as_deref().unwrap_or("unknown"),
        cluster_name,
        default_namespace.as_deref().unwrap_or("none"),
        cred.token.is_some(),
        cred.cert_data.is_some(),
    );

    let mut facts = FactsUpdate::default();
    facts.new_entities.push(Box::new(cred));
    facts.new_entities.push(Box::new(cluster));
    facts.new_relations.push(Box::new(AuthenticatesTo::new(
        cred_id.clone(),
        cluster_id.clone(),
    )));
    if !source_id.is_empty() {
        facts
            .new_relations
            .push(Box::new(Contains::new(source_id, cred_id)));
    }
    if let Some(namespace_name) = default_namespace {
        let namespace = Namespace::new(namespace_name);
        let namespace_id = namespace.entity_id().0.clone();
        facts.new_entities.push(Box::new(namespace));
        facts
            .new_relations
            .push(Box::new(Contains::new(cluster_id, namespace_id)));
    }

    ParserOutput::SuccessWithFacts(facts, detail)
}

/// Parse a `file:content(path)` effect.
///
/// Always records `path` in the caller's system entity `files` list (via the
/// returned `SystemFieldUpdates` embedded in the `ParserOutput`).  Additionally,
/// when the content looks like a kubeconfig, delegates to
/// [`parse_file_kubeconfig`] to create a `K8sCredential` entity.
///
/// Returns:
/// - `SuccessWithFacts` — content is a kubeconfig; credential entity emitted
/// - `Success(SystemFieldUpdates)` — plain file; path recorded in `system.files`
/// - `KnownFailure` — empty stdout
pub(super) fn parse_file_content(stdout: &str, path: &str, source_id: &str) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout for file:content".to_string());
    }

    if is_kubeconfig_content(stdout) {
        // Delegate to the kubeconfig parser — it emits the credential entity.
        // The file path is tracked by the caller in parse_output_effect via
        // apply_system_update before calling us.
        parse_file_kubeconfig(stdout, source_id)
    } else {
        // Plain file: record the path in system.files.
        use crate::external_parser::SystemFieldUpdates;
        ParserOutput::Success(
            SystemFieldUpdates {
                files: vec![path.to_string()],
                ..Default::default()
            },
            format!("stored file path: {} ({} bytes)", path, stdout.len()),
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ran_domain::{AuthenticatesTo, Contains, K8sCluster, K8sCredential, Relation, Uses};

    // Minimal valid kubeconfig YAML with token auth.
    const KUBECONFIG_TOKEN: &str = r#"apiVersion: v1
kind: Config
clusters:
- cluster:
    server: https://10.96.0.1:6443
    certificate-authority-data: LS0tLS1CRUdJTi==
  name: test-cluster
contexts:
- context:
    cluster: test-cluster
    user: admin
    namespace: default
  name: test-context
current-context: test-context
users:
- name: admin
  user:
    token: ya29.supersecrettoken
"#;

    // Kubeconfig with mTLS (cert + key) auth.
    const KUBECONFIG_CERT: &str = r#"apiVersion: v1
kind: Config
clusters:
- cluster:
    server: https://172.16.0.1:6443
    certificate-authority-data: LS0tLS1CRUdJTi==
  name: prod-cluster
contexts:
- context:
    cluster: prod-cluster
    user: admin
  name: prod-context
current-context: prod-context
users:
- name: admin
  user:
    client-certificate-data: CERTDATA==
    client-key-data: KEYDATA==
"#;

    // -----------------------------------------------------------------------
    // extract_path
    // -----------------------------------------------------------------------

    #[test]
    fn extract_path_simple() {
        assert_eq!(extract_path("file:content(/tmp/foo)"), Some("/tmp/foo"));
    }

    #[test]
    fn extract_path_with_colons_and_slashes() {
        assert_eq!(
            extract_path("file:content(/var/run/secrets/kubernetes.io/serviceaccount/token)"),
            Some("/var/run/secrets/kubernetes.io/serviceaccount/token")
        );
    }

    #[test]
    fn extract_path_empty_parens_returns_empty_str() {
        assert_eq!(extract_path("file:content()"), Some(""));
    }

    #[test]
    fn extract_path_no_parens_returns_none() {
        assert_eq!(extract_path("file:content"), None);
    }

    // -----------------------------------------------------------------------
    // is_kubeconfig_content
    // -----------------------------------------------------------------------

    #[test]
    fn is_kubeconfig_true_for_valid_kubeconfig() {
        assert!(is_kubeconfig_content(KUBECONFIG_TOKEN));
    }

    #[test]
    fn is_kubeconfig_false_for_plain_text() {
        assert!(!is_kubeconfig_content("hello world\nthis is a plain file"));
    }

    // -----------------------------------------------------------------------
    // parse_file_kubeconfig
    // -----------------------------------------------------------------------

    #[test]
    fn parse_file_kubeconfig_token_auth() {
        let result = parse_file_kubeconfig(KUBECONFIG_TOKEN, "ns/default/pod/attacker");
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 1);
        let cred = facts.new_entities[0]
            .as_any()
            .downcast_ref::<K8sCredential>()
            .unwrap();
        assert_eq!(cred.endpoint, "https://10.96.0.1:6443");
        assert_eq!(cred.default_namespace.as_deref(), Some("default"));
        assert_eq!(cred.token.as_deref(), Some("ya29.supersecrettoken"));
        assert!(cred.cert_data.is_none());
        assert!(cred.ca_data.is_some());
        // Uses relation emitted
        assert_eq!(facts.new_relations.len(), 1);
        let uses = facts.new_relations[0]
            .as_any()
            .downcast_ref::<Uses>()
            .unwrap();
        assert_eq!(uses.subject_id.0, "ns/default/pod/attacker");
    }

    #[test]
    fn parse_file_kubeconfig_cert_auth() {
        let result = parse_file_kubeconfig(KUBECONFIG_CERT, "ns/default/pod/pwned");
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };
        let cred = facts.new_entities[0]
            .as_any()
            .downcast_ref::<K8sCredential>()
            .unwrap();
        assert_eq!(cred.endpoint, "https://172.16.0.1:6443");
        assert!(cred.token.is_none());
        assert_eq!(cred.cert_data.as_deref(), Some("CERTDATA=="));
        assert_eq!(cred.key_data.as_deref(), Some("KEYDATA=="));
    }

    #[test]
    fn parse_file_kubeconfig_empty_stdout_returns_known_failure() {
        assert!(matches!(
            parse_file_kubeconfig("", "src"),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn parse_file_kubeconfig_malformed_yaml_returns_unknown_format() {
        assert!(matches!(
            parse_file_kubeconfig("{not: yaml: at: all:", "src"),
            ParserOutput::UnknownFormat(_)
        ));
    }

    #[test]
    fn parse_file_kubeconfig_no_source_id_skips_uses_relation() {
        let ParserOutput::SuccessWithFacts(facts, _) = parse_file_kubeconfig(KUBECONFIG_TOKEN, "")
        else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 1);
        assert_eq!(
            facts.new_relations.len(),
            0,
            "no Uses relation when source_id is empty"
        );
    }

    // -----------------------------------------------------------------------
    // parse_file_content
    // -----------------------------------------------------------------------

    #[test]
    fn parse_file_content_plain_text_records_path() {
        let result = parse_file_content("hello world\nsome data", "/tmp/foo", "ns/default/pod/p");
        let ParserOutput::Success(updates, detail) = result else {
            panic!("expected Success, got {:?}", result);
        };
        assert_eq!(updates.files, vec!["/tmp/foo"]);
        assert!(detail.contains("/tmp/foo"));
    }

    #[test]
    fn parse_file_content_kubeconfig_emits_credential() {
        let result = parse_file_content(
            KUBECONFIG_TOKEN,
            "/etc/kubernetes/admin.conf",
            "ns/kube-system/pod/p",
        );
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts for kubeconfig content");
        };
        assert_eq!(facts.new_entities.len(), 1);
        assert!(facts.new_entities[0]
            .as_any()
            .downcast_ref::<K8sCredential>()
            .is_some());
    }

    #[test]
    fn parse_file_content_empty_stdout_returns_known_failure() {
        assert!(matches!(
            parse_file_content("", "/etc/passwd", "src"),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn extract_path_from_nested_path() {
        // Regression: paths with colons shouldn't confuse the extractor.
        let effect = "file:content(/var/run/secrets/token)";
        assert_eq!(extract_path(effect), Some("/var/run/secrets/token"));
    }

    // -----------------------------------------------------------------------
    // parse_local_kubeconfig
    // -----------------------------------------------------------------------

    const OPERATOR_HOST: &str = "system/operator-host";

    #[test]
    fn parse_local_kubeconfig_marks_credential_active_and_emits_cluster() {
        let ParserOutput::SuccessWithFacts(facts, _) =
            parse_local_kubeconfig(KUBECONFIG_TOKEN, OPERATOR_HOST)
        else {
            panic!("expected SuccessWithFacts");
        };

        let cred = facts
            .new_entities
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<K8sCredential>())
            .expect("credential entity emitted");
        assert!(
            cred.active,
            "local kubeconfig establishes the active identity"
        );
        assert_eq!(cred.endpoint, "https://10.96.0.1:6443");
        let cred_id = cred.entity_id().0.clone();

        let cluster = facts
            .new_entities
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<K8sCluster>())
            .expect("cluster entity emitted");
        assert_eq!(cluster.server.as_deref(), Some("https://10.96.0.1:6443"));
        let cluster_id = cluster.entity_id().0.clone();

        // AuthenticatesTo(credential -> cluster)
        assert!(facts.new_relations.iter().any(|r| {
            r.as_any()
                .downcast_ref::<AuthenticatesTo>()
                .is_some_and(|a| a.source_id().0 == cred_id && a.target_id().0 == cluster_id)
        }));

        // Contains(operator-host -> credential)
        assert!(facts.new_relations.iter().any(|r| {
            r.as_any()
                .downcast_ref::<Contains>()
                .is_some_and(|c| c.source_id().0 == OPERATOR_HOST && c.target_id().0 == cred_id)
        }));
    }

    #[test]
    fn parse_local_kubeconfig_names_credential_by_context_not_server() {
        let ParserOutput::SuccessWithFacts(facts, _) =
            parse_local_kubeconfig(KUBECONFIG_TOKEN, OPERATOR_HOST)
        else {
            panic!("expected SuccessWithFacts");
        };
        let cred = facts
            .new_entities
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<K8sCredential>())
            .expect("credential entity emitted");
        // KUBECONFIG_TOKEN's current context is `test-context`.
        assert_eq!(cred.entity_name(), "test-context");
        assert_ne!(cred.entity_name(), cred.endpoint);
        assert_eq!(cred.entity_id().0, "k8s/credential/test-context");
    }

    #[test]
    fn parse_local_kubeconfig_emits_default_namespace_containment() {
        let ParserOutput::SuccessWithFacts(facts, _) =
            parse_local_kubeconfig(KUBECONFIG_TOKEN, OPERATOR_HOST)
        else {
            panic!("expected SuccessWithFacts");
        };
        // KUBECONFIG_TOKEN declares namespace: default on its context.
        assert!(facts
            .new_entities
            .iter()
            .any(|e| e.as_any().downcast_ref::<Namespace>().is_some()));
    }

    #[test]
    fn parse_local_kubeconfig_empty_stdout_is_known_failure() {
        assert!(matches!(
            parse_local_kubeconfig("", OPERATOR_HOST),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn parse_local_kubeconfig_malformed_is_unknown_format() {
        assert!(matches!(
            parse_local_kubeconfig("{not: yaml: at: all:", OPERATOR_HOST),
            ParserOutput::UnknownFormat(_)
        ));
    }
}
