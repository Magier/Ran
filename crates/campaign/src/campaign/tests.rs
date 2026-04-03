use std::collections::HashMap;

use armory::{Procedure, Ttp};
use c2::{ExecTtp, TtpExecuted};
use ran_domain::{Entity, EntityId, K8sCluster, Pod};

use super::Campaign;
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
