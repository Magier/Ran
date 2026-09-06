/// Integration tests for the `app` crate's service layer.
///
/// Tests that do not touch Kubernetes can run anywhere.
/// Tests that spin up a full `AppState` (including `k8s::Client`) are marked
/// `#[ignore]` and require a valid kubeconfig at the default location.
/// Run them with: `cargo test -p app -- --ignored`

// ---------------------------------------------------------------------------
// Config tests — no infrastructure needed
// ---------------------------------------------------------------------------

#[test]
fn namespace_filter_blacklist_excludes_system_namespaces() {
    let filter = app::config::NamespaceFilter::default();
    assert!(!filter.should_include("kube-system"));
    assert!(!filter.should_include("local-path-storage"));
    assert!(filter.should_include("default"));
    assert!(filter.should_include("my-app"));
}

#[test]
fn namespace_filter_whitelist_only_allows_listed() {
    let filter = app::config::NamespaceFilter {
        included: vec!["prod".to_string(), "staging".to_string()],
        excluded: vec!["kube-system".to_string()],
    };
    assert!(filter.should_include("prod"));
    assert!(filter.should_include("staging"));
    // Whitelist takes precedence — "kube-system" is in excluded but included is
    // non-empty, so only whitelisted entries pass.
    assert!(!filter.should_include("kube-system"));
    assert!(!filter.should_include("default"));
}

#[test]
fn config_load_returns_defaults_when_file_missing() {
    let result = app::config::load(Some(std::path::PathBuf::from("/nonexistent/ran.yaml")));
    // Missing file → Ok with defaults, not an error.
    let cfg = result.expect("missing file should return defaults");
    assert!(!cfg.namespaces.excluded.is_empty());
    assert!(cfg.namespaces.included.is_empty());
}

#[test]
fn config_loads_seed_knowledge_and_resolves_relative_path() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("ran.yaml");
    std::fs::write(
        &config_path,
        r#"seedKnowledge:
  - type: credential
    credentialType: kubeconfig
    id: developer
    path: fixtures/developer.yaml
    provenance: scenario
"#,
    )
    .unwrap();

    let config = app::config::load(Some(config_path)).unwrap();
    let app::config::SeedKnowledgeConfig::Credential(seed) = &config.seed_knowledge[0] else {
        panic!("expected credential seed")
    };
    assert_eq!(seed.path, tmp.path().join("fixtures/developer.yaml"));
}

#[test]
fn config_rejects_duplicate_seed_ids_and_unknown_credential_types() {
    let tmp = tempfile::tempdir().unwrap();
    let duplicate_path = tmp.path().join("duplicate.yaml");
    std::fs::write(
        &duplicate_path,
        r#"seedKnowledge:
  - type: cluster
    id: duplicate
    provenance: scenario
  - type: cluster
    id: duplicate
    provenance: scenario
"#,
    )
    .unwrap();
    assert!(app::config::load(Some(duplicate_path)).is_err());

    let unsupported_path = tmp.path().join("unsupported.yaml");
    std::fs::write(
        &unsupported_path,
        r#"seedKnowledge:
  - type: credential
    credentialType: token
    id: token
    path: token.yaml
    provenance: scenario
"#,
    )
    .unwrap();
    assert!(app::config::load(Some(unsupported_path)).is_err());
}

// ---------------------------------------------------------------------------
// Full service test — requires kubeconfig
// ---------------------------------------------------------------------------

/// Creates a full `AppState` and exercises the `ApiService` trait methods that
/// do not contact the Kubernetes API server (campaign state, armory).
///
/// Requires a valid kubeconfig at the default location.
/// Run with: `cargo test -p app -- --ignored`
#[tokio::test]
#[ignore = "requires a valid kubeconfig at the default location"]
async fn app_state_get_and_reset_campaign_without_cli() {
    use api::ApiService;
    use std::sync::{Arc, RwLock};

    let kubeconfig = k8s::default_kubeconfig_path();
    let k8s = k8s::Client::from_kubeconfig(Some(kubeconfig.clone()))
        .await
        .expect("failed to create k8s client from default kubeconfig");

    let target_cluster = k8s::target_cluster_from_kubeconfig(Some(kubeconfig))
        .expect("failed to read target cluster from kubeconfig");

    let campaign_cluster = ran_domain::K8sCluster::new(target_cluster.name.clone())
        .with_context_name(target_cluster.context_name)
        .with_server(target_cluster.server);

    let campaign = Arc::new(RwLock::new(campaign::Campaign::bootstrap(
        "Test",
        campaign_cluster.clone(),
    )));

    let (c2_handle, c2_events, c2_manager) = c2::C2Manager::new(32, k8s.clone());
    let campaign_events = campaign::CampaignEventBus::new(32);

    tokio::spawn(c2_manager.run());
    campaign::spawn_c2_event_processor_with_external_parser(
        campaign.clone(),
        c2_events,
        campaign_events.clone(),
        None,
    );

    // Load an empty armory from a temp directory.
    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let armory = armory::Armory::load_from_dir(tmp.path()).expect("failed to load empty armory");

    let state = app::AppState::new(
        k8s,
        campaign,
        c2_handle,
        armory,
        app::config::NamespaceFilter::default(),
        utility_ai::Profile::default(),
        utility_ai::Profile::default(),
        None,
        false,
        false,
        "Test".to_string(),
        campaign::InitialKnowledge {
            clusters: vec![campaign::InitialClusterKnowledge {
                cluster: campaign_cluster,
                provenance: std::collections::BTreeSet::from([
                    campaign::KnowledgeProvenance::Operator,
                ]),
            }],
            kubeconfigs: Vec::new(),
            ..Default::default()
        },
        campaign_events,
        std::path::PathBuf::from("plans"),
        kubetier::Catalog::embedded(),
    );

    // get_campaign — should return a freshly bootstrapped campaign.
    let c = state.get_campaign().await.expect("get_campaign failed");
    assert_eq!(
        c.entity_count(),
        0,
        "fresh campaign should have no entities"
    );

    // get_armory — empty armory, no TTPs.
    let ttps = state
        .get_armory(api::GetArmoryParams { tactic: None })
        .await
        .expect("get_armory failed");
    assert!(ttps.is_empty(), "empty armory should return no TTPs");

    // reset_campaign — should succeed and leave campaign intact.
    state.reset_campaign().await.expect("reset_campaign failed");
    let c = state
        .get_campaign()
        .await
        .expect("get_campaign after reset failed");
    assert_eq!(c.entity_count(), 0);
}
