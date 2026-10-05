use super::*;
use crate::action_resolution::{
    resolve_action, ActionReadinessStatus, ActionResolutionInput, ProcedureReadinessStatus,
};
use crate::campaign::execution_planning::{
    ClientExecutionPlanner, ExecutionPlacement, ProcedureExecutionSemantics,
};
use crate::ttp_applicability::{resolve_target_context, ttp_applicable_for_target};
use ran_domain::NameConfidence;

fn request_action(kind: &str, http: bool) -> Ttp {
    let mut ttp = Ttp::new("request", "Request", "Discovery");
    ttp.requires.insert("kind".into(), serde_json::json!(kind));
    ttp.procedures = vec![Procedure {
        http_request: http.then(|| {
            serde_json::json!({
                "url": "https://api.example/resources", "method": "GET", "use_ca": false
            })
        }),
        k8s_request: (!http).then(|| {
            serde_json::json!({
                "authentication": "${K8S_AUTH}", "api_server": "https://api.example",
                "api": "/api/v1", "resource": "nodes", "cluster_scoped": true,
                "use_ca": false
            })
        }),
        ..Procedure::new("proc-1", "")
    }];
    if !http {
        ttp.params.push(TtpParam {
            name: "K8S_AUTH".into(),
            param_type: "K8sAuth".into(),
            description: "Identity".into(),
            required: true,
            default: String::new(),
            options: vec![],
        });
    }
    ttp
}

fn request_armory(ttp: Ttp) -> Armory {
    let mut definitions = curl_armory().ttps().to_vec();
    definitions.push(ttp);
    Armory::from_ttps(definitions)
}

fn request_fixture(kind: &str) -> (Campaign, String, String, String) {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let mut source = Pod::new("client", "controlled");
    source.system.set_binary("curl", "/usr/local/bin/curl");
    let source_id = source.entity_id().0;
    campaign.entities.insert_typed(source);
    push_exec_edge(&mut campaign, BUILTIN_C2_ID, &source_id);
    let auth_id = insert_test_auth_service_account(&mut campaign);
    let target_id = match kind {
        "Node" => {
            let mut target = K8sNode::new("node-01");
            target
                .system
                .binaries
                .insert("curl".into(), BinaryPresence::Absent);
            let id = target.entity_id().0;
            campaign.entities.insert_typed(target);
            id
        }
        "Pod" => {
            let target = Pod::new("resource", "uncontrolled");
            let id = target.entity_id().0;
            campaign.entities.insert_typed(target);
            id
        }
        "Namespace" => {
            let target = Namespace::new("uncontrolled");
            let id = target.entity_id().0;
            campaign.entities.insert_typed(target);
            id
        }
        "ServiceAccount" => auth_id.clone(),
        _ => panic!("unsupported fixture kind"),
    };
    (campaign, target_id, source_id, auth_id)
}

fn request_for(
    target_id: &str,
    auth_id: Option<&str>,
    executor_id: Option<&str>,
) -> ExecuteActionRequest {
    ExecuteActionRequest {
        action_id: "request".into(),
        target_id: target_id.into(),
        auth_identity_id: auth_id.map(str::to_string),
        exec_system_id: executor_id.map(str::to_string),
        procedure_id: Some("proc-1".into()),
        args: HashMap::new(),
        execution_timeout_seconds: None,
        reasoning: None,
    }
}

#[test]
fn network_requests_route_clients_independently_of_resource_kind_and_authentication() {
    for kind in ["Node", "Pod", "Namespace", "ServiceAccount"] {
        for http in [false, true] {
            let (mut campaign, target_id, source_id, auth_id) = request_fixture(kind);
            let ttp = request_action(kind, http);
            let armory = request_armory(ttp.clone());
            for explicit in [false, true] {
                let resolution = resolve_action(
                    &ttp,
                    &campaign,
                    &target_id,
                    &ActionResolutionInput {
                        auth_identity_id: (!http).then(|| auth_id.clone()),
                        exec_system_id: explicit.then(|| source_id.clone()),
                        procedure_id: Some("proc-1".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(
                    resolution.status,
                    ActionReadinessStatus::Ready,
                    "{kind}, http={http}, explicit={explicit}"
                );
                assert_eq!(
                    resolution.procedures[0].required_tool.as_deref(),
                    Some("curl")
                );
                let exec = campaign
                    .prepare_action(
                        request_for(
                            &target_id,
                            (!http).then_some(auth_id.as_str()),
                            explicit.then_some(source_id.as_str()),
                        ),
                        &armory,
                    )
                    .unwrap();
                assert_eq!(exec.target_id, target_id);
                assert_eq!(exec.exec_chain, vec![source_id.clone()]);
                assert_eq!(exec.args.get("SRC"), Some(&source_id));
                assert!(exec.procedure.command.starts_with("/usr/local/bin/curl"));
                assert!(exec.procedure.k8s_request.is_none());
                assert!(exec.procedure.http_request.is_none());
            }
        }
    }
}

#[test]
fn bundled_node_proxy_request_has_equivalent_implicit_and_explicit_client_routes() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<ServiceAccount>()
        .get_mut(&EntityId::new(&auth_id))
        .unwrap()
        .entitlements
        .push(RbacPermission::new("get", "nodes/proxy"));
    let armory = Armory::load_from_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../armory/TTPs"),
    )
    .unwrap();
    for hint in [None, Some(source_id.as_str())] {
        let mut request = request_for(&target_id, Some(&auth_id), hint);
        request.action_id = "get-pods-via-node-proxy".into();
        let exec = campaign.prepare_action(request, &armory).unwrap();
        assert_eq!(exec.target_id, target_id);
        assert_eq!(exec.exec_chain, vec![source_id.clone()]);
        assert!(exec
            .procedure
            .command
            .contains("/api/v1/nodes/node-01/proxy/pods"));
        assert_eq!(exec.args.get("NODE").map(String::as_str), Some("node-01"));
        assert_eq!(exec.auth_identity_id.as_deref(), Some(auth_id.as_str()));
    }
}

#[test]
fn readiness_reports_missing_client_route_even_when_resource_is_a_system() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign.graph.remove_entity(&EntityId::new(&source_id));
    let ttp = request_action("Node", false);
    let target = resolve_target_context(&campaign, &target_id).unwrap();
    assert!(!ttp_applicable_for_target(&ttp, &campaign, &target));
    let resolution = resolve_action(
        &ttp,
        &campaign,
        &target_id,
        &ActionResolutionInput {
            auth_identity_id: Some(auth_id.clone()),
            procedure_id: Some("proc-1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(resolution.status, ActionReadinessStatus::Ready);
    assert_eq!(
        resolution.procedures[0].status,
        ProcedureReadinessStatus::Unavailable
    );
    assert!(resolution.procedures[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("client execution system"));
    let mut preview_ttp = ttp.clone();
    preview_ttp.params.push(TtpParam {
        name: "CLIENT_NAME".into(),
        param_type: "string".into(),
        default: "${SRC.NAME}".into(),
        description: String::new(),
        required: false,
        options: vec![],
    });
    let preview = resolve_action(
        &preview_ttp,
        &campaign,
        &target_id,
        &ActionResolutionInput {
            auth_identity_id: Some(auth_id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        preview
            .arguments
            .iter()
            .find(|argument| argument.name == "CLIENT_NAME")
            .unwrap()
            .value
            .as_deref(),
        Some("${SRC.NAME}")
    );
    assert!(
        matches!(campaign.prepare_action(request_for(&target_id, Some(&auth_id), None), &request_armory(ttp)),
        Err(ExecuteActionError::NoExecChannel(reason)) if reason.contains("client execution system"))
    );
}

#[test]
fn client_selection_uses_active_sessions_without_requiring_graph_reconciliation() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign.graph.remove_entity(&EntityId::new(&source_id));
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .sessions
        .push(SessionInfo {
            id: "client-session".into(),
            kind: "tcp".into(),
            port: Some(4444),
            status: SessionStatus::Active,
        });
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), None),
            &request_armory(request_action("Node", false)),
        )
        .unwrap();
    assert_eq!(exec.exec_system_id, "session/client-session");
    assert_eq!(exec.exec_chain, vec![source_id]);
}
#[test]
fn review_explicit_target_placement_is_preserved_for_http() {
    let (mut campaign, target_id, source_id, _) = request_fixture("Pod");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&target_id))
        .unwrap()
        .system
        .set_binary("curl", "/target/curl");
    push_exec_edge(&mut campaign, BUILTIN_C2_ID, &target_id);
    let mut ttp = request_action("Pod", true);
    ttp.procedures[0].run_on_target = Some(true);
    let exec = campaign
        .prepare_action(request_for(&target_id, None, None), &request_armory(ttp))
        .unwrap();
    assert_eq!(
        exec.exec_chain,
        vec![target_id],
        "target placement must not choose unrelated source {source_id}"
    );
}

#[test]
fn review_native_client_does_not_silently_ignore_explicit_executor() {
    let (mut campaign, target_id, source_id, _) = request_fixture("Node");
    let mut credential = K8sCredential::new("https://api.example").with_name("local-client");
    credential.active = true;
    let auth_id = credential.entity_id().0;
    campaign.entities.insert_typed(credential);
    let result = campaign.prepare_action(
        request_for(&target_id, Some(&auth_id), Some(&source_id)),
        &request_armory(request_action("Node", false)),
    );
    assert!(result.is_err(), "local-only credentials should reject an incompatible explicit remote executor, got {result:?}");
}

#[test]
fn review_wrapping_uses_the_unbroken_edge_chosen_by_search() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .binaries
        .insert("curl".into(), BinaryPresence::Absent);
    let mut client = Pod::new("downstream", "controlled");
    client.system.set_binary("curl", "/opt/curl");
    let client_id = client.entity_id().0;
    campaign.entities.insert_typed(client);
    let live = cortex::edge::EdgeData::new("rce.can-exec", 2.5, true)
        .with_envelope(Some("SAFE '${CMD}'".into()));
    let mut broken = cortex::edge::EdgeData::new("container.escape", 2.0, true)
        .with_envelope(Some("BROKEN '${CMD}'".into()));
    broken.broken = true;
    campaign
        .graph
        .insert_edge(&EntityId::new(&source_id), &EntityId::new(&client_id), live);
    campaign.graph.insert_edge(
        &EntityId::new(&source_id),
        &EntityId::new(&client_id),
        broken,
    );
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), None),
            &request_armory(request_action("Node", false)),
        )
        .unwrap();
    assert!(
        exec.procedure.command.starts_with("SAFE "),
        "selected a broken edge: {}",
        exec.procedure.command
    );
}

#[test]
fn review_local_kubectl_does_not_imply_resource_pod_is_running() {
    let (mut campaign, target_id, _, _) = request_fixture("Pod");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&target_id))
        .unwrap()
        .is_running = false;
    let mut credential = K8sCredential::new("https://api.example").with_name("local-client");
    credential.active = true;
    let auth_id = credential.entity_id().0;
    campaign.entities.insert_typed(credential);
    let mut ttp = request_action("Pod", false);
    ttp.procedures[0].k8s_request = None;
    ttp.procedures[0].command = "kubectl ${K8S_AUTH} get pods -o json".into();
    ttp.procedures[0].tool = Some("kubectl".into());
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), None),
            &request_armory(ttp),
        )
        .unwrap();
    assert!(matches!(
        exec.operation,
        ExecutionOperation::KubernetesCommand { .. }
    ));
    campaign
        .on_ttp_executed(&exec, &sample_event("{}"))
        .unwrap();
    let pod = campaign
        .entities
        .find::<Pod>(&EntityId::new(&target_id))
        .unwrap();
    assert!(
        !pod.is_running,
        "operator-side kubectl falsely marked the resource pod as running"
    );
    assert_eq!(pod.system.has_binary("kubectl"), BinaryPresence::Unknown);
}

#[test]
fn review_ready_route_has_a_realizable_envelope() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .binaries
        .insert("curl".into(), BinaryPresence::Absent);
    let mut client = Pod::new("downstream", "controlled");
    client.system.set_binary("curl", "/opt/curl");
    let client_id = client.entity_id().0;
    campaign.entities.insert_typed(client);
    push_relation(&mut campaign, &RceCanExec::new(&source_id, &client_id));
    let ttp = request_action("Node", false);
    let resolution = resolve_action(
        &ttp,
        &campaign,
        &target_id,
        &ActionResolutionInput {
            auth_identity_id: Some(auth_id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(
        resolution.status,
        ActionReadinessStatus::Ready,
        "planner declared an envelope-less RCE executable"
    );
}

#[test]
fn enumeration_shares_one_search_and_rejects_a_different_campaign_snapshot() {
    let (campaign, target, _, _) = request_fixture("Node");
    let planner = ClientExecutionPlanner::new(&campaign);
    for _ in 0..100 {
        crate::action_resolution::resolve_action_with_context(
            &request_action("Node", true),
            &campaign,
            &target,
            &ActionResolutionInput::default(),
            &planner,
        )
        .unwrap();
    }
    assert_eq!(planner.graph_search_count(), 1);
    let different = campaign.clone();
    assert!(crate::action_resolution::resolve_action_with_context(
        &request_action("Node", true),
        &different,
        &target,
        &ActionResolutionInput::default(),
        &planner
    )
    .is_none());
}

#[test]
fn ambient_local_client_keeps_its_identity_and_rejects_known_absent_tools() {
    let (mut campaign, target, _, _) = request_fixture("Pod");
    let mut ttp = request_action("Pod", false);
    ttp.params.clear();
    ttp.procedures[0].k8s_request = None;
    ttp.procedures[0].is_local_command = Some(true);
    ttp.procedures[0].tool = Some("kubectl".into());
    ttp.procedures[0].command = "kubectl get pods".into();
    let armory = request_armory(ttp.clone());
    let exec = campaign
        .prepare_action(
            request_for(&target, None, Some(ran_domain::OPERATOR_HOST_ID)),
            &armory,
        )
        .unwrap();
    assert!(exec.auth_identity_id.is_none());
    assert_eq!(
        exec.execution_environment
            .as_ref()
            .unwrap()
            .system_id
            .as_deref(),
        Some(ran_domain::OPERATOR_HOST_ID)
    );
    campaign
        .entities
        .get_mut::<OperatorHost>()
        .get_mut(&EntityId::new(ran_domain::OPERATOR_HOST_ID))
        .unwrap()
        .system
        .binaries
        .insert("kubectl".into(), BinaryPresence::Absent);
    let resolution =
        resolve_action(&ttp, &campaign, &target, &ActionResolutionInput::default()).unwrap();
    assert_ne!(resolution.status, ActionReadinessStatus::Ready);
    assert!(campaign
        .prepare_action(request_for(&target, None, None), &armory)
        .is_err());
}

#[test]
fn invalidated_session_metadata_cannot_seed_a_client_route() {
    let (mut campaign, target, source, auth) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source))
        .unwrap()
        .system
        .sessions
        .push(SessionInfo {
            id: "stale".into(),
            kind: "tcp".into(),
            port: None,
            status: SessionStatus::Active,
        });
    campaign
        .graph
        .activate_session_on_incoming_exec(&EntityId::new(&source), "session/stale".into());
    assert_eq!(campaign.graph.mark_session_broken("session/stale"), 1);
    assert!(campaign
        .prepare_action(
            request_for(&target, Some(&auth), None),
            &request_armory(request_action("Node", false))
        )
        .is_err());
}

#[test]
fn known_absent_transport_tool_does_not_produce_ready_candidates() {
    let (mut campaign, target, source, auth) = request_fixture("Node");
    let system = &mut campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source))
        .unwrap()
        .system;
    system
        .binaries
        .insert("curl".into(), BinaryPresence::Absent);
    system
        .binaries
        .insert("redis-cli".into(), BinaryPresence::Absent);
    let mut client = Pod::new("downstream", "default");
    client.system.set_binary("curl", "/bin/curl");
    let id = client.entity_id().0;
    campaign.entities.insert_typed(client);
    push_relation(
        &mut campaign,
        &RceCanExec::new(source, id).with_envelope("redis-cli ${CMD}"),
    );
    let ttp = request_action("Node", false);
    let resolution = resolve_action(
        &ttp,
        &campaign,
        &target,
        &ActionResolutionInput {
            auth_identity_id: Some(auth),
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(resolution.status, ActionReadinessStatus::Ready);
}

#[test]
fn kubelet_transit_does_not_infer_host_access_but_escape_does() {
    let (mut campaign, target, source, _) = request_fixture("Node");
    let mut update = crate::FactsUpdate::default();
    update
        .new_relations
        .push(Box::new(ran_domain::KubeletExecSource::new(
            &source, &target,
        )));
    let inferred =
        crate::InferenceRule::infer(&crate::analyzers::CanExecAccessAnalyzer, &campaign, &update);
    assert!(inferred.new_entities.is_empty());
    update.new_relations.clear();
    update.new_relations.push(Box::new(
        ran_domain::ContainerEscape::new(&source, &target).with_envelope("escape ${CMD}"),
    ));
    let inferred =
        crate::InferenceRule::infer(&crate::analyzers::CanExecAccessAnalyzer, &campaign, &update);
    campaign.apply_facts(&inferred);
    assert_eq!(
        campaign
            .get_system_entity(&target)
            .unwrap()
            .entity()
            .system()
            .access_level,
        AccessLevel::Exec
    );
}

#[test]
fn legacy_unknown_execution_location_does_not_fall_back_to_semantic_resource() {
    let (mut campaign, target, _, _) = request_fixture("Pod");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&target))
        .unwrap()
        .is_running = false;
    let mut command = sample_exec_ttp(&target, vec![]);
    command.execution_environment = None;
    assert!(command.execution_environment.is_none());
    campaign
        .on_ttp_executed(&command, &sample_event("{}"))
        .unwrap();
    let pod = campaign
        .entities
        .find::<Pod>(&EntityId::new(&target))
        .unwrap();
    assert!(!pod.is_running);
    assert_eq!(pod.system.has_binary("env"), BinaryPresence::Unknown);
}

#[test]
fn execution_environment_roundtrips_without_legacy_target_fallback() {
    let (mut campaign, target, source, _) = request_fixture("Node");
    let exec = campaign
        .prepare_action(
            request_for(&target, None, None),
            &request_armory(request_action("Node", true)),
        )
        .unwrap();
    let record = crate::ExecutionRecord::from_execution(&exec, &sample_event("{}"));
    let mut value = serde_json::to_value(&record).unwrap();
    assert_eq!(value["execution_environment"]["system_id"], source);
    assert_eq!(value["execution_environment"]["tool"], "curl");
    value
        .as_object_mut()
        .unwrap()
        .remove("execution_environment");
    let legacy: crate::ExecutionRecord = serde_json::from_value(value).unwrap();
    assert!(legacy.execution_environment.is_none());
}

#[test]
#[ignore = "manual action-enumeration benchmark"]
fn action_enumeration_benchmark() {
    for systems in [100usize, 500] {
        let (mut campaign, target, source, _) = request_fixture("Node");
        for number in 0..systems {
            let mut client = Pod::new(format!("client-{number:04}"), "benchmark");
            client.system.set_binary("curl", "/bin/curl");
            let id = client.entity_id().0;
            campaign.entities.insert_typed(client);
            push_exec_edge(&mut campaign, &source, &id);
            if number > 0 {
                for previous in number.saturating_sub(4)..number {
                    push_exec_edge(
                        &mut campaign,
                        &format!("ns/benchmark/pod/client-{previous:04}"),
                        &id,
                    );
                }
            }
        }
        let actions = (0..200)
            .map(|number| {
                let mut ttp = request_action("Node", true);
                ttp.id = format!("action-{number}");
                ttp
            })
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        for action in &actions {
            resolve_action(
                action,
                &campaign,
                &target,
                &ActionResolutionInput::default(),
            )
            .unwrap();
        }
        let fresh = started.elapsed();
        let planner = ClientExecutionPlanner::new(&campaign);
        let started = std::time::Instant::now();
        for action in &actions {
            crate::action_resolution::resolve_action_with_context(
                action,
                &campaign,
                &target,
                &ActionResolutionInput::default(),
                &planner,
            )
            .unwrap();
        }
        let shared = started.elapsed();
        assert_eq!(planner.graph_search_count(), 1);
        eprintln!(
            "systems={systems}, actions=200, fresh={fresh:?}, shared={shared:?}, searches=200->1"
        );
    }
}

#[test]
fn review_kubelet_capability_is_not_a_node_shell() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .binaries
        .insert("curl".into(), BinaryPresence::Absent);
    campaign
        .entities
        .get_mut::<K8sNode>()
        .get_mut(&EntityId::new(&target_id))
        .unwrap()
        .system
        .set_binary("curl", "/node/curl");
    push_relation(
        &mut campaign,
        &ran_domain::KubeletExecSource::new(&source_id, &target_id),
    );
    let result = campaign.prepare_action(
        request_for(&target_id, Some(&auth_id), None),
        &request_armory(request_action("Node", false)),
    );
    assert!(
        result.is_err(),
        "transit-only Node must not become a client executor"
    );
}

#[test]
fn client_selection_prefers_tool_evidence_on_a_multi_hop_executor() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .binaries
        .insert("curl".into(), BinaryPresence::Absent);
    let mut client = Pod::new("downstream", "controlled");
    client.system.set_binary("curl", "/opt/curl");
    let client_id = client.entity_id().0;
    campaign.entities.insert_typed(client);
    push_relation(
        &mut campaign,
        &RceCanExec::new(&source_id, &client_id).with_envelope("remote-run '${CMD}'"),
    );
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), None),
            &request_armory(request_action("Node", false)),
        )
        .unwrap();
    assert_eq!(exec.exec_chain, vec![source_id, client_id.clone()]);
    assert_eq!(exec.args.get("SRC"), Some(&client_id));
    assert!(exec.procedure.command.contains("/opt/curl"));
}

#[test]
fn native_client_request_preserves_resource_target_without_remote_executor() {
    let (mut campaign, target_id, source_id, _) = request_fixture("Node");
    campaign.graph.remove_entity(&EntityId::new(&source_id));
    let mut credential = K8sCredential::new("https://api.example").with_name("local-client");
    credential.active = true;
    let auth_id = credential.entity_id().0;
    campaign.entities.insert_typed(credential);
    let ttp = request_action("Node", false);
    let resolution = resolve_action(
        &ttp,
        &campaign,
        &target_id,
        &ActionResolutionInput {
            auth_identity_id: Some(auth_id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(resolution.status, ActionReadinessStatus::Ready);
    assert!(resolution.procedures[0].required_tool.is_none());
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), None),
            &request_armory(ttp),
        )
        .unwrap();
    assert_eq!(exec.target_id, target_id);
    assert!(exec.exec_chain.is_empty());
    assert!(matches!(
        exec.operation,
        ExecutionOperation::KubernetesRequest { .. }
    ));
}

#[test]
fn selected_authentication_binding_controls_readiness_and_procedure_selection() {
    let (mut campaign, target_id, source_id, token_id) = request_fixture("Node");
    campaign.graph.remove_entity(&EntityId::new(&source_id));
    let mut credential = K8sCredential::new("https://api.example").with_name("local-client");
    credential.active = true;
    campaign.entities.insert_typed(credential);
    let ttp = request_action("Node", false);
    let resolution = resolve_action(
        &ttp,
        &campaign,
        &target_id,
        &ActionResolutionInput {
            auth_identity_id: Some(token_id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(resolution.status, ActionReadinessStatus::Blocked);
    assert!(resolution.recommended_procedure_id.is_none());
    assert!(matches!(
        campaign.prepare_action(
            request_for(&target_id, Some(&token_id), None),
            &request_armory(ttp)
        ),
        Err(ExecuteActionError::NoExecChannel(_))
    ));
}

#[test]
fn client_may_run_on_the_resource_system_when_explicitly_selected_and_reachable() {
    let (mut campaign, target_id, _, auth_id) = request_fixture("Pod");
    push_exec_edge(&mut campaign, BUILTIN_C2_ID, &target_id);
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), Some(&target_id)),
            &request_armory(request_action("Pod", false)),
        )
        .unwrap();
    assert_eq!(exec.exec_chain, vec![target_id.clone()]);
    assert_eq!(exec.target_id, target_id);
}

#[test]
fn explicit_client_alias_is_canonicalized_and_unreachable_hint_does_not_fall_back() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    let stale = EntityId::new("sys/old-client");
    campaign.record_entity_alias(&stale, &EntityId::new(&source_id));
    let armory = request_armory(request_action("Node", false));
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), Some(&stale.0)),
            &armory,
        )
        .unwrap();
    assert_eq!(exec.exec_chain, vec![source_id]);
    assert!(matches!(
        campaign.prepare_action(
            request_for(&target_id, Some(&auth_id), Some(&target_id)),
            &armory
        ),
        Err(ExecuteActionError::NoExecChannel(_))
    ));
}

#[test]
fn unavailable_client_tool_blocks_without_using_resource_binary_evidence() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .binaries
        .insert("curl".into(), BinaryPresence::Absent);
    campaign
        .entities
        .get_mut::<K8sNode>()
        .get_mut(&EntityId::new(&target_id))
        .unwrap()
        .system
        .set_binary("curl", "/bin/curl");
    let ttp = request_action("Node", false);
    let resolution = resolve_action(
        &ttp,
        &campaign,
        &target_id,
        &ActionResolutionInput::default(),
    )
    .unwrap();
    assert_ne!(resolution.status, ActionReadinessStatus::Ready);
    assert!(resolution.procedures[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("tool 'curl'"));
    assert!(
        matches!(campaign.prepare_action(request_for(&target_id, Some(&auth_id), None), &request_armory(ttp)),
        Err(ExecuteActionError::InvalidInput(reason)) if reason.contains("tool 'curl'"))
    );
}

#[test]
fn client_source_properties_and_resource_properties_ground_independently() {
    let (mut campaign, target_id, source_id, _) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .ips
        .push("10.0.0.10".parse().unwrap());
    campaign
        .entities
        .get_mut::<K8sNode>()
        .get_mut(&EntityId::new(&target_id))
        .unwrap()
        .system
        .ips
        .push("10.0.0.20".parse().unwrap());
    let mut ttp = request_action("Node", true);
    for (name, default) in [("CLIENT_IP", "${SRC.IP}"), ("RESOURCE_IP", "${TARGET.IP}")] {
        ttp.params.push(TtpParam {
            name: name.into(),
            param_type: "string".into(),
            default: default.into(),
            description: String::new(),
            required: true,
            options: vec![],
        });
    }
    let resolution = resolve_action(
        &ttp,
        &campaign,
        &target_id,
        &ActionResolutionInput::default(),
    )
    .unwrap();
    assert_eq!(resolution.status, ActionReadinessStatus::Ready);
    let exec = campaign
        .prepare_action(request_for(&target_id, None, None), &request_armory(ttp))
        .unwrap();
    assert_eq!(
        exec.args.get("CLIENT_IP").map(String::as_str),
        Some("10.0.0.10")
    );
    assert_eq!(
        exec.args.get("RESOURCE_IP").map(String::as_str),
        Some("10.0.0.20")
    );
}

#[test]
fn source_properties_wait_for_authentication_that_selects_the_client_environment() {
    let (mut campaign, target_id, _, token_id) = request_fixture("Node");
    let mut credential = K8sCredential::new("https://api.example").with_name("local-client");
    credential.active = true;
    let credential_id = credential.entity_id().0;
    campaign.entities.insert_typed(credential);
    let mut ttp = request_action("Node", false);
    ttp.params.push(TtpParam {
        name: "CLIENT_NAME".into(),
        param_type: "string".into(),
        default: "${SRC.NAME}".into(),
        description: String::new(),
        required: true,
        options: vec![],
    });
    let unbound = resolve_action(
        &ttp,
        &campaign,
        &target_id,
        &ActionResolutionInput::default(),
    )
    .unwrap();
    let source_argument = unbound
        .arguments
        .iter()
        .find(|argument| argument.name == "CLIENT_NAME")
        .unwrap();
    assert_eq!(source_argument.value.as_deref(), Some("${SRC.NAME}"));
    assert_ne!(unbound.status, ActionReadinessStatus::Ready);
    for (auth_id, expected_name) in [(token_id, "client"), (credential_id, "Operator host")] {
        let bound = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput {
                auth_identity_id: Some(auth_id),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(bound.status, ActionReadinessStatus::Ready, "{bound:#?}");
        assert_eq!(
            bound
                .arguments
                .iter()
                .find(|argument| argument.name == "CLIENT_NAME")
                .unwrap()
                .value
                .as_deref(),
            Some(expected_name)
        );
    }
}

#[test]
fn planning_is_payload_independent_and_unchanged_snapshots_are_deterministic() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    let ttp = request_action("Node", false);
    let definition = &ttp.procedures[0];
    let intent = ProcedureExecutionSemantics::from_definition(definition);
    assert_eq!(intent.placement, ExecutionPlacement::Client);
    let planner = ClientExecutionPlanner::new(&campaign);
    let before = planner
        .plan(&ttp, definition, &target_id, Some(&auth_id), None)
        .unwrap();
    let mut rendered = definition.clone();
    materialize_k8s_request(&mut rendered, &curl_armory(), Some("test-token")).unwrap();
    assert!(rendered.k8s_request.is_none());
    let after = planner
        .plan(&ttp, definition, &target_id, Some(&auth_id), None)
        .unwrap();
    assert_eq!(before.channel, after.channel);
    assert_eq!(before.executor_id(), Some(source_id.as_str()));
    drop(planner);
    let armory = request_armory(ttp);
    let first = campaign
        .prepare_action(request_for(&target_id, Some(&auth_id), None), &armory)
        .unwrap();
    let second = campaign
        .prepare_action(request_for(&target_id, Some(&auth_id), None), &armory)
        .unwrap();
    assert_eq!(first.exec_chain, second.exec_chain);
    assert_eq!(first.procedure.command, second.procedure.command);
}

#[test]
fn client_can_reach_downstream_executor_from_a_session_only_origin() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign.graph.remove_entity(&EntityId::new(&source_id));
    let source = campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap();
    source
        .system
        .binaries
        .insert("curl".into(), BinaryPresence::Absent);
    source.system.sessions.push(SessionInfo {
        id: "origin".into(),
        kind: "tcp".into(),
        port: Some(4444),
        status: SessionStatus::Active,
    });
    let mut downstream = Pod::new("downstream", "controlled");
    downstream.system.set_binary("curl", "/opt/curl");
    let downstream_id = downstream.entity_id().0;
    campaign.entities.insert_typed(downstream);
    push_relation(
        &mut campaign,
        &RceCanExec::new(&source_id, &downstream_id).with_envelope("remote-run ${CMD}"),
    );
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), None),
            &request_armory(request_action("Node", false)),
        )
        .unwrap();
    assert_eq!(exec.exec_system_id, "session/origin");
    assert_eq!(exec.exec_chain, vec![source_id, downstream_id]);
}

#[test]
fn alternative_source_finds_a_longer_route_that_avoids_the_semantic_target() {
    let (mut campaign, target_id, source_id, _) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .binaries
        .insert("curl".into(), BinaryPresence::Absent);
    let mut destination = Pod::new("destination", "controlled");
    destination.system.set_binary("curl", "/opt/curl");
    let destination_id = destination.entity_id().0;
    campaign.entities.insert_typed(destination);
    for (source, target) in [(&source_id, &target_id), (&target_id, &destination_id)] {
        push_relation(
            &mut campaign,
            &RceCanExec::new(source, target).with_envelope("remote-run ${CMD}"),
        );
    }
    let mut previous = source_id.clone();
    let mut safe_path = vec![source_id];
    for name in ["safe-a", "safe-b"] {
        let mut hop = Pod::new(name, "controlled");
        hop.system
            .binaries
            .insert("curl".into(), BinaryPresence::Absent);
        let id = hop.entity_id().0;
        campaign.entities.insert_typed(hop);
        push_relation(
            &mut campaign,
            &RceCanExec::new(&previous, &id).with_envelope("remote-run ${CMD}"),
        );
        safe_path.push(id.clone());
        previous = id;
    }
    push_relation(
        &mut campaign,
        &RceCanExec::new(&previous, &destination_id).with_envelope("remote-run ${CMD}"),
    );
    safe_path.push(destination_id.clone());
    let mut ttp = request_action("Node", true);
    ttp.procedures[0].run_on_target = Some(false);
    let exec = campaign
        .prepare_action(
            request_for(&target_id, None, Some(&destination_id)),
            &request_armory(ttp),
        )
        .unwrap();
    assert_eq!(exec.exec_chain, safe_path);
    assert!(!exec.exec_chain.contains(&target_id));
}

#[test]
fn adapter_alternatives_are_selected_using_executor_tools() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    let source = campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap();
    source
        .system
        .binaries
        .insert("curl".into(), BinaryPresence::Absent);
    source.system.set_binary("wget", "/opt/wget");
    let mut ttp = request_action("Node", false);
    let mut wget = ttp.procedures[0].clone();
    wget.id = "wget".into();
    wget.tool = Some("wget".into());
    ttp.procedures.push(wget);
    let mut definitions = curl_armory().ttps().to_vec();
    definitions.push(Ttp {
        tool_slot: Some("http-request".into()), status: "disabled".into(),
        procedures: vec![Procedure::new("wget", "wget -qO- '${URL}' {% for name, value in HEADERS %} --header=\"{{ name }}: {{ value }}\" {% endfor %}")],
        ..Ttp::new("wget", "wget", "Execution")
    });
    definitions.push(ttp.clone());
    let armory = Armory::from_ttps(definitions);
    let resolution = resolve_action(
        &ttp,
        &campaign,
        &target_id,
        &ActionResolutionInput::default(),
    )
    .unwrap();
    assert_eq!(resolution.status, ActionReadinessStatus::Ready);
    assert_eq!(resolution.recommended_procedure_id.as_deref(), Some("wget"));
    let mut request = request_for(&target_id, Some(&auth_id), None);
    request.procedure_id = None;
    let exec = campaign.prepare_action(request, &armory).unwrap();
    assert_eq!(exec.procedure.id, "wget");
    assert!(exec.procedure.command.starts_with("/opt/wget"));
}

#[test]
fn a_legacy_backend_hint_constrains_the_client_environment() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign.graph.remove_entity(&EntityId::new(&source_id));
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .sessions
        .push(SessionInfo {
            id: "selected".into(),
            kind: "tcp".into(),
            port: Some(4444),
            status: SessionStatus::Active,
        });
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), Some("session/selected")),
            &request_armory(request_action("Node", false)),
        )
        .unwrap();
    assert_eq!(exec.exec_system_id, "session/selected");
    assert_eq!(exec.exec_chain, vec![source_id]);
}

#[test]
fn adapter_execution_evidence_belongs_to_the_client_not_the_resource() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .binaries
        .remove("curl");
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), None),
            &request_armory(request_action("Node", false)),
        )
        .unwrap();
    assert_eq!(exec.procedure.tool.as_deref(), Some("curl"));
    campaign
        .on_ttp_executed(&exec, &sample_event("{}"))
        .unwrap();
    assert!(matches!(
        campaign
            .get_system_entity(&source_id)
            .unwrap()
            .entity()
            .system()
            .has_binary("curl"),
        BinaryPresence::Present(_)
    ));
    assert_eq!(
        campaign
            .get_system_entity(&target_id)
            .unwrap()
            .entity()
            .system()
            .has_binary("curl"),
        BinaryPresence::Absent
    );
}

#[test]
fn native_api_response_does_not_imply_execution_or_binaries_on_a_pod_resource() {
    let (mut campaign, target_id, _, _) = request_fixture("Pod");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&target_id))
        .unwrap()
        .is_running = false;
    let mut credential = K8sCredential::new("https://api.example").with_name("local-client");
    credential.active = true;
    let auth_id = credential.entity_id().0;
    campaign.entities.insert_typed(credential);
    let mut ttp = request_action("Pod", false);
    ttp.procedures[0].tool = Some("curl".into());
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), None),
            &request_armory(ttp),
        )
        .unwrap();
    campaign
        .on_ttp_executed(&exec, &sample_event("{}"))
        .unwrap();
    let pod = campaign
        .entities
        .find::<Pod>(&EntityId::new(&target_id))
        .unwrap();
    assert!(!pod.is_running);
    assert_eq!(pod.system.has_binary("curl"), BinaryPresence::Unknown);
}

#[test]
fn unknown_client_tool_remains_runnable_after_its_channel_is_proven() {
    let (mut campaign, target_id, source_id, auth_id) = request_fixture("Node");
    campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&source_id))
        .unwrap()
        .system
        .binaries
        .remove("curl");
    let ttp = request_action("Node", false);
    let resolution = resolve_action(
        &ttp,
        &campaign,
        &target_id,
        &ActionResolutionInput::default(),
    )
    .unwrap();
    assert_eq!(resolution.status, ActionReadinessStatus::Ready);
    assert_eq!(
        resolution.procedures[0].status,
        ProcedureReadinessStatus::Unknown
    );
    let exec = campaign
        .prepare_action(
            request_for(&target_id, Some(&auth_id), None),
            &request_armory(ttp),
        )
        .unwrap();
    assert_eq!(exec.exec_chain, vec![source_id]);
}

fn local_shell_fixture() -> (Campaign, Armory, String, String) {
    let (campaign, target, source, _) = request_fixture("Pod");
    let mut ttp = Ttp::new("request", "Local shell", "Discovery");
    ttp.requires.insert("kind".into(), serde_json::json!("Pod"));
    let mut procedure = Procedure::new("proc-1", "printf ok");
    procedure.is_local_command = Some(true);
    ttp.procedures.push(procedure);
    ttp.effects
        .push("sys.has-binary(review-proof, /local/review-proof)".into());
    (campaign, Armory::from_ttps(vec![ttp]), target, source)
}

#[test]
fn review_local_shell_placement_is_independent_of_request_format_or_auth() {
    let (mut campaign, armory, target, source) = local_shell_fixture();
    let exec = campaign
        .prepare_action(request_for(&target, None, None), &armory)
        .unwrap();
    assert!(matches!(
        exec.operation,
        c2::ExecutionOperation::LocalShell { .. }
    ));
    assert!(exec.exec_chain.is_empty());
    assert_eq!(
        exec.execution_environment
            .as_ref()
            .unwrap()
            .system_id
            .as_deref(),
        Some("system/operator-host")
    );
    campaign
        .on_ttp_executed(&exec, &sample_event("ok"))
        .unwrap();
    for id in [&source, &target] {
        assert_eq!(
            campaign
                .get_system_entity(id)
                .unwrap()
                .entity()
                .system()
                .has_binary("review-proof"),
            BinaryPresence::Unknown
        );
    }
    assert_eq!(
        campaign
            .get_system_entity("system/operator-host")
            .unwrap()
            .entity()
            .system()
            .has_binary("review-proof"),
        BinaryPresence::Present("/local/review-proof".into())
    );
}

#[test]
fn review_local_shell_rejects_remote_executor_and_conflicting_target_placement() {
    let (mut campaign, armory, target, source) = local_shell_fixture();
    assert!(campaign
        .prepare_action(request_for(&target, None, Some(&source)), &armory)
        .is_err());
    let mut ttp = armory.ttps()[0].clone();
    ttp.procedures[0].run_on_target = Some(true);
    assert!(campaign
        .prepare_action(
            request_for(&target, None, None),
            &Armory::from_ttps(vec![ttp])
        )
        .is_err());
    let exec = campaign
        .prepare_action(
            request_for(&target, None, Some("system/operator-host")),
            &armory,
        )
        .unwrap();
    assert!(exec.exec_chain.is_empty());
}

#[test]
fn review_parsed_system_evidence_ignores_remote_chain_and_semantic_target() {
    let (mut campaign, armory, target, source) = local_shell_fixture();
    let mut exec = campaign
        .prepare_action(request_for(&target, None, None), &armory)
        .unwrap();
    // Simulate a historical or externally supplied display chain. It cannot
    // override persisted physical executor provenance.
    exec.exec_chain = vec![source.clone()];
    campaign
        .on_ttp_executed(&exec, &sample_event("ok"))
        .unwrap();
    assert_eq!(
        campaign
            .get_system_entity(&source)
            .unwrap()
            .entity()
            .system()
            .has_binary("review-proof"),
        BinaryPresence::Unknown
    );
    assert_eq!(
        campaign
            .get_system_entity("system/operator-host")
            .unwrap()
            .entity()
            .system()
            .has_binary("review-proof"),
        BinaryPresence::Present("/local/review-proof".into())
    );
}

#[test]
fn physical_capability_effects_cannot_borrow_a_semantic_target_or_submitted_executor() {
    let (mut campaign, armory, target, source) = local_shell_fixture();
    let mut exec = campaign
        .prepare_action(request_for(&target, None, None), &armory)
        .unwrap();
    exec.ttp.effects = vec!["k8s.kubelet-exec(sys, all(k8s.Node))".into()];
    exec.args.insert("EXECUTOR_ID".into(), source.clone());
    exec.exec_chain = vec![source.clone()];
    let node = K8sNode::new("worker");
    let node_id = node.entity_id().0;
    campaign.entities.insert_typed(node);
    let processed = campaign
        .on_ttp_executed(&exec, &sample_event("ok"))
        .unwrap();
    let marker = processed
        .updates
        .new_relations
        .iter()
        .find(|r| r.relation_name() == "kubelet-exec-capability")
        .unwrap();
    assert_eq!(marker.source_id().0, "system/operator-host");
    for id in [&target, &source] {
        assert!(campaign
            .graph
            .targets_of(&EntityId::new(id), "kubelet-exec")
            .is_empty());
    }
    assert!(campaign.resolve_exec_channel(&node_id).is_err());
}

#[test]
fn review_kubelet_capability_discovery_reaches_pod_with_typed_realization() {
    let (mut campaign, target, source, _) = request_fixture("Pod");
    let mut node = K8sNode::new("worker");
    node.system.ips.push("2001:db8::1".parse().unwrap());
    let node_id = node.entity_id().0;
    campaign.entities.insert_typed(node);
    let mut account = ServiceAccount::new("ambient", "controlled");
    account
        .entitlements
        .push(RbacPermission::new("get", "nodes/proxy"));
    let account_id = account.entity_id().0;
    campaign.entities.insert_typed(account);
    push_relation(&mut campaign, &Uses::new(&source, &account_id));
    let pod = campaign
        .entities
        .get_mut::<Pod>()
        .get_mut(&EntityId::new(&target))
        .unwrap();
    pod.is_running = true;
    pod.system.set_binary("sh", "/bin/sh");
    pod.containers.push(Container {
        name: "worker-container".into(),
        image: "test".into(),
        args: vec![],
        ports: vec![],
        volume_mounts: vec![],
    });
    push_relation(&mut campaign, &RunsOn::new(&target, &node_id));
    let installed = sample_exec_ttp(
        &source,
        vec![
            "sys.has-binary(ranplant, /tmp/ranplant)",
            "k8s.kubelet-exec(sys, all(k8s.Node))",
        ],
    );
    campaign
        .on_ttp_executed(&installed, &sample_event("ok"))
        .unwrap();
    let channel = campaign.resolve_exec_channel(&target).unwrap();
    assert_eq!(channel.kubelet_plans.len(), 1);
    assert!(channel
        .edges
        .iter()
        .all(|edge| edge.data.envelope.is_none()));
    assert!(campaign.resolve_exec_channel(&node_id).is_err());
    assert_eq!(
        campaign
            .get_system_entity(&node_id)
            .unwrap()
            .entity()
            .system()
            .access_level,
        AccessLevel::None
    );
    let mut ttp = armory_with_command("request", "printf '%s' 'a & b'; id", None).ttps()[0].clone();
    ttp.effects.push("sys.envvar".into());
    let armory = Armory::from_ttps(vec![ttp]);
    let mut request = request_for(&target, None, None);
    request.procedure_id = None;
    let exec = campaign.prepare_action(request, &armory).unwrap();
    assert_eq!(
        exec.output_transform,
        Some(OutputTransformKind::JsonEnvelope)
    );
    assert_eq!(
        exec.execution_environment
            .as_ref()
            .unwrap()
            .system_id
            .as_deref(),
        Some(target.as_str())
    );
    let words = shell_words::split(&exec.procedure.command).unwrap();
    assert_eq!(&words[..2], &["/tmp/ranplant", "kubelet-exec"]);
    let url_index = words.iter().position(|word| word == "--url").unwrap() + 1;
    let url = url::Url::parse(&words[url_index]).unwrap();
    assert_eq!(url.host_str(), Some("[2001:db8::1]"));
    assert_eq!(url.path(), "/exec/uncontrolled/resource/worker-container");
    let argv = url
        .query_pairs()
        .filter(|(key, _)| key == "command")
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    assert_eq!(argv, ["/bin/sh", "-c", "printf '%s' 'a & b'; id"]);
    campaign
        .on_ttp_executed(&exec, &sample_event(r#"{"result":"KUBELET_PROOF=1"}"#))
        .unwrap();
    assert_eq!(
        campaign
            .get_system_entity(&target)
            .unwrap()
            .entity()
            .system()
            .env_vars
            .get("KUBELET_PROOF")
            .map(String::as_str),
        Some("1")
    );
    for id in [&source, &node_id] {
        assert!(!campaign
            .get_system_entity(id)
            .unwrap()
            .entity()
            .system()
            .env_vars
            .contains_key("KUBELET_PROOF"));
    }
}

#[test]
fn review_transport_failure_with_unknown_stage_does_not_write_binary_facts() {
    let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
    let mut source = Pod::new("source", "controlled");
    source.system.set_binary("ranplant", "/tmp/ranplant");
    let source_id = source.entity_id().0;
    campaign.entities.insert_typed(source);
    push_exec_edge(&mut campaign, BUILTIN_C2_ID, &source_id);

    let mut target = Pod::new("target", "other");
    target.containers.push(Container {
        name: "main".into(),
        image: String::new(),
        args: vec![],
        ports: vec![],
        volume_mounts: vec![],
    });
    let target_id = target.entity_id().0;
    campaign.entities.insert_typed(target);
    let node = K8sNode::new("worker");
    let node_id = node.entity_id().0;
    campaign.entities.insert_typed(node);
    campaign.upsert_relation(
        &ran_domain::KubeletExecSource::new(&source_id, &node_id),
        ran_domain::KnowledgeProvenance::Action,
    );
    campaign.upsert_relation(
        &ran_domain::KubeletExecSink::new(&node_id, &target_id),
        ran_domain::KnowledgeProvenance::Action,
    );

    let armory = armory_with_command("request", "printf ok", None);
    let mut request = request_for(&target_id, None, None);
    request.procedure_id = None;
    let exec = campaign.prepare_action(request, &armory).unwrap();
    assert_eq!(
        exec.transport_environment
            .as_ref()
            .and_then(|environment| environment.system_id.as_deref()),
        Some(source_id.as_str())
    );
    assert_eq!(
        exec.transport_environment
            .as_ref()
            .and_then(|environment| environment.tool.as_deref()),
        Some("ranplant")
    );

    // Wrapper names, payload names, and unclassified errors are all ambiguous.
    // Cover both explicit failures and zero-exit failures detected in output,
    // including replay records that have no transport_environment field.
    for legacy in [false, true] {
        let mut exec = exec.clone();
        if legacy {
            exec.transport_environment = None;
        }
        for binary in ["ranplant", "printf", "curl"] {
            for success in [false, true] {
                let message = format!("sh: 1: {binary}: not found");
                let event = c2::TtpExecuted {
                    id: exec.id.clone(),
                    success,
                    results: vec![message.clone()],
                    exit_code: if success { 0 } else { 127 },
                    fail_reason: if success { String::new() } else { message },
                    session_connected: None,
                };
                let outcome = campaign.on_ttp_executed(&exec, &event).unwrap();
                assert!(!outcome.effective_success);
                for id in [&source_id, &node_id, &target_id] {
                    let expected = if id == &source_id && binary == "ranplant" {
                        BinaryPresence::Present("/tmp/ranplant".into())
                    } else {
                        BinaryPresence::Unknown
                    };
                    assert_eq!(
                        campaign
                            .get_system_entity(id)
                            .unwrap()
                            .entity()
                            .system()
                            .has_binary(binary),
                        expected,
                        "legacy={legacy}, success={success}, host={id}, binary={binary}"
                    );
                }
            }
        }
    }
}

#[test]
fn review_structural_effect_metadata_comes_from_the_persisted_executor() {
    let (mut campaign, target_id, source_id, _) = request_fixture("Pod");
    for (id, node_name) in [(&source_id, "source-host"), (&target_id, "target-host")] {
        let mut pod = campaign
            .entities
            .find::<Pod>(&EntityId::new(id))
            .unwrap()
            .clone();
        pod.node_name = Some(node_name.into());
        campaign.entities.insert_typed(pod);
    }
    let mut ttp = Ttp::new("escape", "Escape", "Execution");
    ttp.requires.insert("kind".into(), serde_json::json!("Pod"));
    let mut procedure = Procedure::new("proc-1", "printf ok");
    procedure.run_on_target = Some(false);
    ttp.procedures.push(procedure);
    ttp.effects.push("container.escape(sys)".into());
    let armory = Armory::from_ttps(vec![ttp]);
    let mut request = request_for(&target_id, None, None);
    request.action_id = "escape".into();

    let mut exec = campaign.prepare_action(request, &armory).unwrap();
    assert_eq!(
        exec.execution_environment
            .as_ref()
            .and_then(|environment| environment.system_id.as_deref()),
        Some(source_id.as_str())
    );
    // Even submitted reserved context cannot change the executor's host.
    exec.args
        .insert("TARGET_NODE_ID".into(), "node/target-host".into());
    exec.args
        .insert("TARGET_NODE_AUTHORITATIVE".into(), "false".into());
    campaign
        .on_ttp_executed(&exec, &sample_event("ok"))
        .unwrap();
    assert_eq!(
        campaign.relation_targets(&EntityId::new(&source_id), "container.escape"),
        vec![EntityId::new("node/source-host")]
    );
    assert_eq!(
        campaign
            .entities
            .find::<K8sNode>(&EntityId::new("node/source-host"))
            .unwrap()
            .name_confidence,
        NameConfidence::Authoritative
    );
}

#[test]
fn escape_host_context_uses_executor_relations_or_a_non_authoritative_placeholder() {
    for hosts in [0, 1] {
        let (mut campaign, target_id, source_id, _) = request_fixture("Pod");
        campaign
            .entities
            .get_mut::<Pod>()
            .get_mut(&EntityId::new(&target_id))
            .unwrap()
            .node_name = Some("target-host".into());
        for name in ["source-host", "conflicting-host"].iter().take(hosts) {
            let node = K8sNode::new(*name);
            let node_id = node.entity_id();
            campaign.entities.insert_typed(node);
            campaign.upsert_relation(
                &ran_domain::RunsOn::new(&source_id, &node_id.0),
                ran_domain::KnowledgeProvenance::Action,
            );
        }
        // Relation insertion also synchronizes Pod metadata. Clear that cache
        // to exercise the graph-only fallback.
        campaign
            .entities
            .get_mut::<Pod>()
            .get_mut(&EntityId::new(&source_id))
            .unwrap()
            .node_name = None;
        let mut ttp = Ttp::new("escape", "Escape", "Execution");
        ttp.requires.insert("kind".into(), serde_json::json!("Pod"));
        let mut procedure = Procedure::new("proc-1", "printf ok");
        procedure.run_on_target = Some(false);
        ttp.procedures.push(procedure);
        ttp.effects.push("container.escape(sys)".into());
        let mut request = request_for(&target_id, None, None);
        request.action_id = "escape".into();
        let mut exec = campaign
            .prepare_action(request, &Armory::from_ttps(vec![ttp]))
            .unwrap();
        // Preparation runs inference, which may repopulate the metadata.
        campaign
            .entities
            .get_mut::<Pod>()
            .get_mut(&EntityId::new(&source_id))
            .unwrap()
            .node_name = None;
        exec.args
            .insert("TARGET_NODE_ID".into(), "node/target-host".into());
        exec.args
            .insert("TARGET_NODE_AUTHORITATIVE".into(), "true".into());
        campaign
            .on_ttp_executed(&exec, &sample_event("ok"))
            .unwrap();
        let expected = if hosts == 1 {
            "node/source-host"
        } else {
            "node/escape_client"
        };
        assert_eq!(
            campaign.relation_targets(&EntityId::new(&source_id), "container.escape"),
            vec![EntityId::new(expected)]
        );
        assert_eq!(
            campaign
                .entities
                .find::<K8sNode>(&EntityId::new(expected))
                .unwrap()
                .name_confidence,
            NameConfidence::Derived
        );
    }
}

#[test]
fn unrealizable_discovered_kubelet_transports_do_not_hide_longer_valid_routes() {
    for failure in [
        "missing-ranplant",
        "missing-shell",
        "no-mounted-token",
        "invalid-endpoint",
    ] {
        let (mut campaign, target, source, _) = request_fixture("Pod");
        campaign
            .entities
            .get_mut::<Pod>()
            .get_mut(&EntityId::new(&source))
            .unwrap()
            .system
            .set_binary("ranplant", "/tmp/ranplant");
        let target_pod = campaign
            .entities
            .get_mut::<Pod>()
            .get_mut(&EntityId::new(&target))
            .unwrap();
        target_pod.system.set_binary("curl", "/usr/bin/curl");
        target_pod.system.set_binary("sh", "/bin/sh");
        let node = K8sNode::new(if failure == "invalid-endpoint" {
            "bad-host/path"
        } else {
            "worker"
        });
        let node_id = node.entity_id().0;
        campaign.entities.insert_typed(node);
        match failure {
            "missing-ranplant" => {
                campaign
                    .entities
                    .get_mut::<Pod>()
                    .get_mut(&EntityId::new(&source))
                    .unwrap()
                    .system
                    .binaries
                    .insert("ranplant".into(), BinaryPresence::Absent);
            }
            "missing-shell" => {
                campaign
                    .entities
                    .get_mut::<Pod>()
                    .get_mut(&EntityId::new(&target))
                    .unwrap()
                    .system
                    .binaries
                    .insert("sh".into(), BinaryPresence::Absent);
            }
            "no-mounted-token" => {
                campaign
                    .entities
                    .get_mut::<Pod>()
                    .get_mut(&EntityId::new(&source))
                    .unwrap()
                    .automount_service_account_token = ran_domain::Confidence::No;
            }
            _ => {}
        }
        push_relation(
            &mut campaign,
            &ran_domain::KubeletExecSource::new(&source, &node_id),
        );
        push_relation(
            &mut campaign,
            &ran_domain::KubeletExecSink::new(&node_id, &target),
        );
        let middle = Pod::new("alternative", "controlled");
        let middle_id = middle.entity_id().0;
        campaign.entities.insert_typed(middle);
        for (from, to) in [(&source, &middle_id), (&middle_id, &target)] {
            push_relation(
                &mut campaign,
                &RceCanExec::new(from, to).with_envelope("remote-run ${CMD}"),
            );
        }
        let mut ttp = request_action("Pod", true);
        ttp.procedures[0].run_on_target = Some(true);
        let resolution =
            resolve_action(&ttp, &campaign, &target, &ActionResolutionInput::default()).unwrap();
        assert_eq!(
            resolution.procedures[0].status,
            ProcedureReadinessStatus::Ready,
            "{failure}"
        );
        let exec = campaign
            .prepare_action(request_for(&target, None, None), &request_armory(ttp))
            .unwrap();
        assert_eq!(exec.exec_chain, [source, middle_id, target], "{failure}");
        assert!(exec.output_transform.is_none());
    }
}
