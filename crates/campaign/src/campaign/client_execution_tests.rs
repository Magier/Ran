use super::*;
use crate::action_resolution::{
    resolve_action, ActionReadinessStatus, ActionResolutionInput, ProcedureReadinessStatus,
};
use crate::campaign::execution_planning::{
    ClientExecutionPlanner, ExecutionPlacement, ProcedureExecutionSemantics,
};
use crate::ttp_applicability::{resolve_target_context, ttp_applicable_for_target};

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
