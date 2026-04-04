use std::collections::HashMap;

use armory::{Armory, Procedure, Ttp};
use c2::{ExecTtp, TtpExecuted};
use ran_domain::{AccessLevel, Entity, EntityId, K8sCluster, Pod, RelationSummary};

use super::{Campaign, ExecChannel, ExecuteActionError, ExecuteActionRequest};
use crate::failure_analyzers::FAILURE_ANALYZER_EFFECT_ID;
use crate::ParseResult;

fn sample_exec_ttp(target_id: &str, effects: Vec<&str>) -> ExecTtp {
    ExecTtp {
        id: "cmd-1".to_string(),
        ttp: Ttp {
            id: "ttp-test".to_string(),
            name: "Test TTP".to_string(),
            description: "test".to_string(),
            tactic: "Discovery".to_string(),
            techniques: vec![],
            status: "stable".to_string(),
            params: vec![],
            requires: Default::default(),
            effects: effects.into_iter().map(str::to_string).collect(),
            procedures: vec![Procedure {
                id: "shell".to_string(),
                command: "env".to_string(),
                tool: None,
                is_local_command: None,
            }],
            references: vec![],
        },
        procedure: Procedure {
            id: "shell".to_string(),
            command: "env".to_string(),
            tool: None,
            is_local_command: None,
        },
        args: HashMap::new(),
        target_id: target_id.to_string(),
        exec_system_id: String::new(),
    }
}

fn sample_event(stdout: &str) -> TtpExecuted {
    TtpExecuted {
        id: "evt-1".to_string(),
        success: true,
        results: vec![stdout.to_string(), String::new()],
        exit_code: 0,
        fail_reason: String::new(),
    }
}

fn sample_failed_event(fail_reason: &str) -> TtpExecuted {
    TtpExecuted {
        id: "evt-1".to_string(),
        success: false,
        results: vec![fail_reason.to_string()],
        exit_code: 1,
        fail_reason: fail_reason.to_string(),
    }
}

#[test]
fn bootstrap_contains_c2_and_cluster_entities() {
    let campaign = Campaign::bootstrap(
        "Ran",
        K8sCluster::new("dev-cluster")
            .with_context_name(Some("dev-context".to_string()))
            .with_server(Some("https://127.0.0.1:6443".to_string())),
    );

    assert_eq!(campaign.entity_count(), 2);
    assert!(campaign.c2_servers.contains_key(&EntityId::new("c2/ran")));
    assert!(campaign
        .clusters
        .contains_key(&EntityId::new("k8s/cluster/dev-cluster")));
}

#[test]
fn on_ttp_executed_records_no_parser_audit_for_unknown_effect() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0;
    campaign.pods.insert(pod.entity_id(), pod);

    let cmd = sample_exec_ttp(&target_id, vec!["sys.unknown"]);
    let event = sample_event("X=1\n");

    let processed = campaign.on_ttp_executed(&cmd, &event).unwrap();

    assert_eq!(processed.parse_audits.len(), 1);
    assert!(matches!(
        processed.parse_audits[0].parse_result,
        ParseResult::NoParser
    ));
}

#[test]
fn on_ttp_executed_parses_sys_envvar_into_target_system_info() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0;
    campaign.pods.insert(pod.entity_id(), pod);

    let cmd = sample_exec_ttp(&target_id, vec!["sys.envvar"]);
    let event = sample_event("HOME=/root\nPATH=/usr/bin\n");

    let processed = campaign.on_ttp_executed(&cmd, &event).unwrap();

    assert_eq!(processed.parse_audits.len(), 1);
    assert!(matches!(
        processed.parse_audits[0].parse_result,
        ParseResult::Parsed
    ));

    let target = campaign.get_system_entity(&target_id).unwrap();
    let env_vars = &target.entity().system().env_vars;
    assert_eq!(env_vars.get("HOME"), Some(&"/root".to_string()));
    assert_eq!(env_vars.get("PATH"), Some(&"/usr/bin".to_string()));
}

#[test]
fn on_ttp_executed_failure_records_known_failure_audit() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
    let cmd = sample_exec_ttp("ns/default/pod", vec!["sys.envvar"]);
    let event = sample_failed_event(
        "invalid pod target id 'ns/default/pod': expected format ns/<namespace>/pod/<pod-name>",
    );

    let processed = campaign.on_ttp_executed(&cmd, &event).unwrap();

    assert_eq!(processed.parse_audits.len(), 1);
    assert!(matches!(
        processed.parse_audits[0].parse_result,
        ParseResult::KnownFailure
    ));
    assert_eq!(processed.parse_audits[0].effect_id, FAILURE_ANALYZER_EFFECT_ID);
}

#[test]
fn on_ttp_executed_failure_records_unknown_format_when_unclassified() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
    let cmd = sample_exec_ttp("ns/default/pod/demo", vec!["sys.envvar"]);
    let event = sample_failed_event("something odd happened");

    let processed = campaign.on_ttp_executed(&cmd, &event).unwrap();

    assert_eq!(processed.parse_audits.len(), 1);
    assert!(matches!(
        processed.parse_audits[0].parse_result,
        ParseResult::UnknownFormat
    ));
    assert_eq!(processed.parse_audits[0].effect_id, FAILURE_ANALYZER_EFFECT_ID);
}

// ---------------------------------------------------------------------------
// resolve_exec_channel tests
// ---------------------------------------------------------------------------

fn can_exec_relation(source_id: &str, target_id: &str) -> RelationSummary {
    RelationSummary {
        name: "k8s.can-exec".to_string(),
        source_id: source_id.to_string(),
        target_id: target_id.to_string(),
    }
}

fn kubelet_pod_exec_relation(source_id: &str, target_id: &str) -> RelationSummary {
    RelationSummary {
        name: "kubelet-pod-exec".to_string(),
        source_id: source_id.to_string(),
        target_id: target_id.to_string(),
    }
}

#[test]
fn resolve_exec_channel_returns_builtin_for_can_exec_relation() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("target", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.pods.insert(pod.entity_id(), pod);
    campaign.relations.push(can_exec_relation("sa/default/some-sa", &target_id));

    let ch = campaign.resolve_exec_channel(&target_id).expect("should find channel");
    assert_eq!(ch, ExecChannel::direct("c2/ran"));
}

#[test]
fn resolve_exec_channel_returns_builtin_for_kubelet_pod_exec_relation() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("target", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.pods.insert(pod.entity_id(), pod);
    campaign.relations.push(kubelet_pod_exec_relation("node/node-a", &target_id));

    let ch = campaign.resolve_exec_channel(&target_id).expect("should find channel");
    assert_eq!(ch, ExecChannel::direct("c2/ran"));
}

#[test]
fn resolve_exec_channel_returns_via_compromised_intermediate() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // Compromised pod (has exec foothold)
    let mut attacker = Pod::new("attacker", "default");
    attacker.system.access_level = AccessLevel::UserExec;
    let attacker_id = attacker.entity_id().0.clone();
    campaign.pods.insert(attacker.entity_id(), attacker);

    // Target pod (no direct exec edge from C2)
    let target = Pod::new("target", "default");
    let target_id = target.entity_id().0.clone();
    campaign.pods.insert(target.entity_id(), target);

    // Attacker → target via k8s.can-exec
    campaign.relations.push(can_exec_relation(&attacker_id, &target_id));

    let ch = campaign.resolve_exec_channel(&target_id).expect("should find channel");
    assert_eq!(ch, ExecChannel::via("c2/ran", &attacker_id));
}

#[test]
fn resolve_exec_channel_resolves_via_service_account_uses_relation() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // Pod that uses the SA and has a direct exec channel
    let pod = Pod::new("player-pod", "dungeon");
    let pod_id = pod.entity_id().0.clone();
    campaign.pods.insert(pod.entity_id(), pod);

    let sa_id = "ns/dungeon/sa/player";
    campaign.relations.push(can_exec_relation("sa/default/ran", &pod_id));
    campaign.relations.push(RelationSummary {
        name: "uses".to_string(),
        source_id: pod_id.clone(),
        target_id: sa_id.to_string(),
    });

    let ch = campaign.resolve_exec_channel(sa_id).expect("should resolve via pod uses SA");
    assert_eq!(ch.backend_id, "c2/ran");
    assert_eq!(ch.exec_target_id.as_deref(), Some(pod_id.as_str()), "exec_target_id must be the pod, not the SA");
}

#[test]
fn resolve_exec_channel_errors_when_no_path_in_graph() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("orphan", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.pods.insert(pod.entity_id(), pod);

    let result = campaign.resolve_exec_channel(&target_id);
    assert!(result.is_err(), "expected Err when no exec relations exist");
}

// ---------------------------------------------------------------------------
// prepare_action channel resolution tests
// ---------------------------------------------------------------------------

fn minimal_armory(ttp_id: &str) -> Armory {
    Armory::from_ttps(vec![Ttp {
        id: ttp_id.to_string(),
        name: "Test TTP".to_string(),
        description: String::new(),
        tactic: "Discovery".to_string(),
        techniques: vec![],
        status: "stable".to_string(),
        params: vec![],
        requires: Default::default(),
        effects: vec![],
        procedures: vec![Procedure {
            id: "shell".to_string(),
            command: "id".to_string(),
            tool: None,
            is_local_command: None,
        }],
        references: vec![],
    }])
}

fn action_request(target_id: &str, exec_system_id: Option<&str>) -> ExecuteActionRequest {
    ExecuteActionRequest {
        action_id: "test-ttp".to_string(),
        target_id: target_id.to_string(),
        exec_system_id: exec_system_id.map(str::to_string),
        procedure_id: None,
        args: HashMap::new(),
    }
}

#[test]
fn prepare_action_auto_resolves_channel_from_graph() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.pods.insert(pod.entity_id(), pod);
    campaign.relations.push(can_exec_relation("sa/default/ran", &target_id));

    let armory = minimal_armory("test-ttp");
    let exec = campaign
        .prepare_action(action_request(&target_id, None), &armory)
        .expect("should prepare action");

    assert_eq!(exec.exec_system_id, "c2/ran");
}

#[test]
fn prepare_action_errors_when_no_exec_channel_in_graph() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.pods.insert(pod.entity_id(), pod);
    // No exec relations added

    let armory = minimal_armory("test-ttp");
    let result = campaign.prepare_action(action_request(&target_id, None), &armory);

    assert!(
        matches!(result, Err(ExecuteActionError::NoExecChannel(_))),
        "expected NoExecChannel error, got {:?}",
        result
    );
}

#[test]
fn prepare_action_respects_caller_supplied_exec_system_id() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.pods.insert(pod.entity_id(), pod);
    // No exec relations — would normally error, but caller supplies explicit backend

    let armory = minimal_armory("test-ttp");
    let exec = campaign
        .prepare_action(action_request(&target_id, Some("custom-backend")), &armory)
        .expect("explicit exec_system_id should bypass graph check");

    assert_eq!(exec.exec_system_id, "custom-backend");
}
