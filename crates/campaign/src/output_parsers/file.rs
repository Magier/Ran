use std::collections::{HashMap, HashSet};

use super::ParserOutput;
use crate::FactsUpdate;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ran_domain::{
    AuthenticatesTo, Contains, Entity, GCPServiceAccount, K8sCluster, K8sCredential, Namespace,
    ServiceAccount, Uses,
};

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

fn is_k8s_service_account_token(content: &str) -> bool {
    content.lines().map(str::trim).any(|line| {
        let mut parts = line.split('.');
        let (Some(_header), Some(payload), Some(_signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        let Ok(payload) = URL_SAFE_NO_PAD.decode(payload) else {
            return false;
        };
        let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&payload) else {
            return false;
        };
        payload
            .get("sub")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|subject| subject.starts_with("system:serviceaccount:"))
            || payload.get("kubernetes.io").is_some()
    })
}

fn is_gcp_service_account_key(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .is_some_and(|value| {
            value.get("type").and_then(serde_json::Value::as_str) == Some("service_account")
                && value.get("client_email").is_some()
                && value.get("private_key").is_some()
        })
}

// ---------------------------------------------------------------------------
// Kubeconfig YAML parsing
// ---------------------------------------------------------------------------

/// Build a `K8sCredential` entity from an already-resolved kubeconfig context.
///
/// This is the single, canonical mapping from a resolved context to a
/// credential entity, shared by the output parser and by the app-side
/// per-context client registry so that both derive the **same** entity id for
/// the same context. The credential is named by its context (a raw server URL
/// is an unfriendly display name / id), falling back to the user name, then the
/// endpoint. `active` is left `false` for the caller to set.
pub fn credential_from_resolved(resolved: &k8s::ResolvedKubeconfig) -> K8sCredential {
    let mut cred = K8sCredential::new(resolved.server.clone().unwrap_or_default());
    cred.context_name = Some(resolved.context_name.clone());
    cred.default_namespace = resolved.default_namespace.clone();
    cred.user_name = resolved.user_name.clone();
    cred.auth_method = resolved.auth_method.clone();
    cred.has_token = resolved.has_token;
    cred.has_client_certificate = resolved.has_client_certificate;
    cred.has_client_key = resolved.has_client_key;
    cred.ca_data = resolved.ca_data.clone();
    cred.token = resolved.token.clone();
    cred.cert_data = resolved.cert_data.clone();
    cred.key_data = resolved.key_data.clone();

    if let Some(label) = non_empty(&resolved.context_name)
        .or_else(|| resolved.user_name.as_deref().and_then(non_empty))
    {
        cred.name = label.to_string();
    }
    cred
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

// ---------------------------------------------------------------------------
// Public parser entry points (called from parse_output_effect with source_id)
// ---------------------------------------------------------------------------

/// Parse kubeconfig YAML and emit every context as a `K8sCredential`, the
/// referenced clusters, and the relations between the source, credentials,
/// clusters, and default namespaces.
///
/// Called for both `file:kubeconfig` (explicit) and the kubeconfig branch of
/// `file:content(...)`.
///
/// Returns:
/// - `SuccessWithFacts` - credential and cluster facts emitted
/// - `KnownFailure` - empty content
/// - `UnknownFormat` - non-empty content that fails YAML parsing or has no cluster entry
pub(super) fn parse_file_kubeconfig(stdout: &str, source_id: &str) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout for file:kubeconfig".to_string());
    }

    let contexts = match k8s::resolve_all_kubeconfig_contexts_yaml(stdout) {
        Ok(contexts) if !contexts.is_empty() => contexts,
        _ => {
            return ParserOutput::UnknownFormat(
                "could not resolve any context from kubeconfig YAML".to_string(),
            )
        }
    };

    let mut facts = FactsUpdate::default();
    let mut emitted_clusters = HashSet::new();
    let mut emitted_namespaces = HashSet::new();
    let mut labels = Vec::new();
    for resolved in &contexts {
        let credential = credential_from_resolved(resolved);
        let credential_id = credential.entity_id().0;
        labels.push(credential.entity_name().to_string());
        facts.new_entities.push(Box::new(credential));

        let mut cluster = K8sCluster::new(&resolved.cluster_name);
        cluster.context_name = Some(resolved.context_name.clone());
        cluster.tls_server_name = resolved.tls_server_name.clone();
        cluster.server = resolved.server.clone().filter(|server| !server.is_empty());
        let cluster_id = cluster.entity_id().0;
        if emitted_clusters.insert(cluster_id.clone()) {
            facts.new_entities.push(Box::new(cluster));
        }
        facts.new_relations.push(Box::new(AuthenticatesTo::new(
            credential_id.clone(),
            cluster_id.clone(),
        )));

        if !source_id.is_empty() {
            facts
                .new_relations
                .push(Box::new(Uses::new(source_id, credential_id)));
        }
        if let Some(namespace_name) = &resolved.default_namespace {
            let namespace = Namespace::new(namespace_name);
            let namespace_id = namespace.entity_id().0;
            if emitted_namespaces.insert(namespace_id.clone()) {
                facts.new_entities.push(Box::new(namespace));
            }
            facts
                .new_relations
                .push(Box::new(Contains::new(cluster_id, namespace_id)));
        }
    }

    let detail = format!(
        "extracted {} kubeconfig context(s) across {} cluster(s): {}",
        contexts.len(),
        emitted_clusters.len(),
        labels.join(", ")
    );

    ParserOutput::SuccessWithFacts(facts, detail)
}

/// Parse the kubeconfig read from the machine running Ran and emit **every**
/// context it defines as a switchable Kubernetes identity.
///
/// Unlike [`parse_file_kubeconfig`] (which records a single knowledge-only
/// credential discovered on some remote system), this reproduces the graph
/// shape Ran used to seed at bootstrap, for each context:
/// - a `K8sCredential`, `active = true` only for the kubeconfig's current
///   context; the others are known-but-inactive identities the operator can
///   switch to via Authenticate As
/// - the `K8sCluster` it authenticates to (deduplicated by entity id)
/// - `AuthenticatesTo(credential → cluster)`
/// - `Contains(source_id → credential)` where `source_id` is the operator host
/// - when the context declares a default namespace, the `Namespace` entity and
///   `Contains(cluster → namespace)`
///
/// `source_id` is the operator-host entity (`system/operator-host`). When empty
/// the containment relation is skipped.
///
/// TODO(tech-debt): this duplicates most of [`parse_file_kubeconfig`]. Kubeconfig
/// parsing is format-invariant - the API server and user identity are inferred
/// the same way regardless of origin. The only real distinction (local/active
/// vs in-cluster discovery) is a *provenance* concern and should drive `active`
/// and cluster-graph emission from a single parser, rather than being encoded as
/// a separate effect + function. See memory
/// `project_kubeconfig_effect_provenance_debt`.
///
/// Returns:
/// - `SuccessWithFacts` - one credential per context, clusters, and relations
/// - `KnownFailure` - empty content
/// - `UnknownFormat` - non-empty content with no resolvable context
pub(super) fn parse_local_kubeconfig(stdout: &str, source_id: &str) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout for file:local-kubeconfig".to_string());
    }

    let contexts = match k8s::resolve_all_kubeconfig_contexts_yaml(stdout) {
        Ok(contexts) if !contexts.is_empty() => contexts,
        _ => {
            return ParserOutput::UnknownFormat(
                "could not resolve any context from local kubeconfig YAML".to_string(),
            )
        }
    };

    let mut facts = FactsUpdate::default();
    let mut emitted_clusters: HashSet<String> = HashSet::new();
    let mut emitted_namespaces: HashSet<String> = HashSet::new();
    let mut credential_labels: Vec<String> = Vec::new();

    for resolved in &contexts {
        let mut cred = credential_from_resolved(resolved);
        cred.active = resolved.is_current_context;
        let cred_id = cred.entity_id().0.clone();
        credential_labels.push(format!(
            "{}{}",
            cred.entity_name(),
            if cred.active { " (active)" } else { "" }
        ));

        let mut cluster = K8sCluster::new(&resolved.cluster_name);
        cluster.context_name = Some(resolved.context_name.clone());
        cluster.tls_server_name = resolved.tls_server_name.clone();
        if let Some(server) = resolved.server.clone().filter(|s| !s.is_empty()) {
            cluster.server = Some(server);
        }
        let cluster_id = cluster.entity_id().0.clone();

        facts.new_entities.push(Box::new(cred));
        if emitted_clusters.insert(cluster_id.clone()) {
            facts.new_entities.push(Box::new(cluster));
        }
        facts.new_relations.push(Box::new(AuthenticatesTo::new(
            cred_id.clone(),
            cluster_id.clone(),
        )));
        if !source_id.is_empty() {
            facts
                .new_relations
                .push(Box::new(Contains::new(source_id, cred_id)));
        }
        if let Some(namespace_name) = resolved.default_namespace.clone() {
            let namespace = Namespace::new(namespace_name);
            let namespace_id = namespace.entity_id().0.clone();
            if emitted_namespaces.insert(namespace_id.clone()) {
                facts.new_entities.push(Box::new(namespace));
            }
            facts
                .new_relations
                .push(Box::new(Contains::new(cluster_id, namespace_id)));
        }
    }

    let detail = format!(
        "established {} local kubeconfig identit{} across {} cluster(s): {}",
        contexts.len(),
        if contexts.len() == 1 { "y" } else { "ies" },
        emitted_clusters.len(),
        credential_labels.join(", "),
    );

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
/// - `SuccessWithFacts` - content is a kubeconfig; credential entity emitted
/// - `Success(SystemFieldUpdates)` - plain file; path recorded in `system.files`
/// - `KnownFailure` - empty stdout
pub(super) fn parse_file_content(
    stdout: &str,
    path: &str,
    source_id: &str,
    args: &HashMap<String, String>,
) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout for file:content".to_string());
    }

    if is_kubeconfig_content(stdout) {
        // Delegate to the kubeconfig parser - it emits the credential entity.
        // The file path is tracked by the caller in parse_output_effect via
        // apply_system_update before calling us.
        parse_file_kubeconfig(stdout, source_id)
    } else if is_k8s_service_account_token(stdout) {
        let mut parser_args = args.clone();
        parser_args.remove("TARGET_ID");
        let mut parsed = super::iam::parse_raw_service_account_token(stdout, "", &parser_args);
        if let ParserOutput::SuccessWithFacts(facts, _) = &mut parsed {
            if !source_id.is_empty() {
                if let Some(account_id) = facts.new_entities.iter().find_map(|entity| {
                    entity
                        .as_any()
                        .downcast_ref::<ServiceAccount>()
                        .map(|account| account.entity_id().0)
                }) {
                    facts
                        .new_relations
                        .push(Box::new(Uses::new(source_id, account_id)));
                }
            }
        }
        parsed
    } else if is_gcp_service_account_key(stdout) {
        let mut parsed = super::gcp::parse_gcp_service_account_key(stdout);
        if let ParserOutput::SuccessWithFacts(facts, _) = &mut parsed {
            if !source_id.is_empty() {
                if let Some(account_id) = facts.new_entities.iter().find_map(|entity| {
                    entity
                        .as_any()
                        .downcast_ref::<GCPServiceAccount>()
                        .map(|account| account.entity_id().0)
                }) {
                    facts
                        .new_relations
                        .push(Box::new(Uses::new(source_id, account_id)));
                }
            }
        }
        parsed
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
    tls-server-name: kubernetes.default.svc
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
        let cred = facts
            .new_entities
            .iter()
            .find_map(|entity| entity.as_any().downcast_ref::<K8sCredential>())
            .unwrap();
        assert_eq!(cred.endpoint, "https://10.96.0.1:6443");
        assert_eq!(cred.default_namespace.as_deref(), Some("default"));
        assert_eq!(cred.token.as_deref(), Some("ya29.supersecrettoken"));
        assert!(cred.cert_data.is_none());
        assert!(cred.ca_data.is_some());
        // Uses relation emitted
        let uses = facts
            .new_relations
            .iter()
            .find_map(|relation| relation.as_any().downcast_ref::<Uses>())
            .unwrap();
        assert_eq!(uses.subject_id.0, "ns/default/pod/attacker");
        assert!(facts.new_relations.iter().any(|relation| relation
            .as_any()
            .downcast_ref::<AuthenticatesTo>()
            .is_some()));
    }

    #[test]
    fn parse_file_kubeconfig_cert_auth() {
        let result = parse_file_kubeconfig(KUBECONFIG_CERT, "ns/default/pod/pwned");
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };
        let cred = facts
            .new_entities
            .iter()
            .find_map(|entity| entity.as_any().downcast_ref::<K8sCredential>())
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
        assert!(!facts
            .new_relations
            .iter()
            .any(|relation| relation.as_any().downcast_ref::<Uses>().is_some()));
    }

    #[test]
    fn parse_file_kubeconfig_emits_every_context_and_cluster_edges() {
        let ParserOutput::SuccessWithFacts(facts, _) =
            parse_file_kubeconfig(KUBECONFIG_MULTI, "ns/default/pod/reader")
        else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(
            facts
                .new_entities
                .iter()
                .filter(|entity| entity.as_any().downcast_ref::<K8sCredential>().is_some())
                .count(),
            2
        );
        assert_eq!(
            facts
                .new_relations
                .iter()
                .filter(|relation| relation
                    .as_any()
                    .downcast_ref::<AuthenticatesTo>()
                    .is_some())
                .count(),
            2
        );
    }

    // -----------------------------------------------------------------------
    // parse_file_content
    // -----------------------------------------------------------------------

    #[test]
    fn parse_file_content_plain_text_records_path() {
        let result = parse_file_content(
            "hello world\nsome data",
            "/tmp/foo",
            "ns/default/pod/p",
            &HashMap::new(),
        );
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
            &HashMap::new(),
        );
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts for kubeconfig content");
        };
        assert!(facts
            .new_entities
            .iter()
            .any(|entity| entity.as_any().downcast_ref::<K8sCredential>().is_some()));
        assert!(facts
            .new_entities
            .iter()
            .any(|entity| entity.as_any().downcast_ref::<K8sCluster>().is_some()));
    }

    #[test]
    fn parse_file_content_empty_stdout_returns_known_failure() {
        assert!(matches!(
            parse_file_content("", "/etc/passwd", "src", &HashMap::new()),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn parse_file_content_service_account_token_emits_account() {
        let payload = URL_SAFE_NO_PAD.encode(
            br#"{"sub":"system:serviceaccount:payments:reader","kubernetes.io":{"namespace":"payments","serviceaccount":{"name":"reader","uid":"sa-1"}}}"#,
        );
        let token = format!("eyJhbGciOiJSUzI1NiJ9.{payload}.signature");
        let result = parse_file_content(
            &token,
            "/var/run/secrets/kubernetes.io/serviceaccount/token",
            "ns/payments/pod/api",
            &HashMap::new(),
        );
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected ServiceAccount facts");
        };
        let account = facts
            .new_entities
            .iter()
            .find_map(|entity| entity.as_any().downcast_ref::<ServiceAccount>())
            .expect("ServiceAccount entity");
        assert_eq!(account.entity_id().0, "ns/payments/sa/reader");
        assert!(account.token.is_some());
        assert!(facts.new_relations.iter().any(|relation| {
            relation
                .as_any()
                .downcast_ref::<Uses>()
                .is_some_and(|uses| uses.subject_id.0 == "ns/payments/pod/api")
        }));
    }

    #[test]
    fn parse_file_content_gcp_key_emits_service_account() {
        let key = r#"{
            "type": "service_account",
            "project_id": "prod-123",
            "private_key_id": "key-id",
            "private_key": "-----BEGIN PRIVATE KEY-----\nsecret\n-----END PRIVATE KEY-----\n",
            "client_email": "runner@prod-123.iam.gserviceaccount.com",
            "client_id": "12345",
            "token_uri": "https://oauth2.googleapis.com/token"
        }"#;
        let result = parse_file_content(
            key,
            "/var/secrets/google/key.json",
            "ns/default/pod/runner",
            &HashMap::new(),
        );
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected GCP service-account facts");
        };
        let account = facts
            .new_entities
            .iter()
            .find_map(|entity| entity.as_any().downcast_ref::<GCPServiceAccount>())
            .expect("GCPServiceAccount entity");
        assert_eq!(account.project.as_deref(), Some("prod-123"));
        assert_eq!(account.private_key_id.as_deref(), Some("key-id"));
        assert!(account.private_key.is_some());
        assert!(facts.new_relations.iter().any(|relation| {
            relation
                .as_any()
                .downcast_ref::<Uses>()
                .is_some_and(|uses| uses.subject_id.0 == "ns/default/pod/runner")
        }));
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

    // Kubeconfig with two contexts against two clusters; prod is current.
    const KUBECONFIG_MULTI: &str = r#"apiVersion: v1
kind: Config
clusters:
- name: prod-cluster
  cluster:
    server: https://prod:6443
- name: staging-cluster
  cluster:
    server: https://staging:6443
contexts:
- name: prod
  context:
    cluster: prod-cluster
    user: prod-admin
    namespace: default
- name: staging
  context:
    cluster: staging-cluster
    user: staging-admin
current-context: prod
users:
- name: prod-admin
  user:
    token: prod-token
- name: staging-admin
  user:
    token: staging-token
"#;

    #[test]
    fn parse_local_kubeconfig_emits_every_context_only_current_active() {
        let ParserOutput::SuccessWithFacts(facts, _) =
            parse_local_kubeconfig(KUBECONFIG_MULTI, OPERATOR_HOST)
        else {
            panic!("expected SuccessWithFacts");
        };

        let creds: Vec<&K8sCredential> = facts
            .new_entities
            .iter()
            .filter_map(|e| e.as_any().downcast_ref::<K8sCredential>())
            .collect();
        assert_eq!(creds.len(), 2, "one credential per context");

        let prod = creds
            .iter()
            .find(|c| c.entity_name() == "prod")
            .expect("prod credential");
        let staging = creds
            .iter()
            .find(|c| c.entity_name() == "staging")
            .expect("staging credential");
        assert!(prod.active, "current context is active");
        assert!(!staging.active, "non-current context is inactive");

        // Two distinct clusters emitted.
        let clusters: Vec<&K8sCluster> = facts
            .new_entities
            .iter()
            .filter_map(|e| e.as_any().downcast_ref::<K8sCluster>())
            .collect();
        assert_eq!(clusters.len(), 2);

        // Both credentials are contained by the operator host.
        let contains_from_host = facts
            .new_relations
            .iter()
            .filter_map(|r| r.as_any().downcast_ref::<Contains>())
            .filter(|c| c.source_id().0 == OPERATOR_HOST)
            .count();
        assert_eq!(contains_from_host, 2);
    }

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
        assert_eq!(
            cluster.tls_server_name.as_deref(),
            Some("kubernetes.default.svc")
        );
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
