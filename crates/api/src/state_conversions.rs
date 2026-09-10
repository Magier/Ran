use std::collections::{HashMap, HashSet};

use campaign::{Campaign, CampaignEntityRef};
use ran_domain::{AccessLevel, Entity, EntityId, K8sCredential, PodPhase};
use serde_json::Value;

use crate::{BootstrapEffect, BootstrapOperation, CampaignState, Graph, GraphEdge, GraphNode};

pub(crate) fn campaign_to_campaign_state(
    campaign: &Campaign,
    kubetier: &kubetier::Catalog,
) -> CampaignState {
    let mut entities = HashMap::new();
    let hosted_services = hosted_app_services(campaign);

    for entity in campaign.get_entities() {
        let id = entity.entity_id().0;
        let mut data = HashMap::new();
        data.insert("id".to_string(), Value::String(id.clone()));
        data.insert(
            "name".to_string(),
            Value::String(entity.entity_name().to_string()),
        );
        data.insert(
            "kind".to_string(),
            Value::String(entity.entity_kind().to_string()),
        );
        if let Some(namespace) = entity.namespace() {
            data.insert(
                "namespace".to_string(),
                Value::String(namespace.to_string()),
            );
        }

        if let Some(mut full_entity) = serialize_campaign_entity_map(&entity) {
            prune_entity_payload_for_ui(entity.entity_kind(), &mut full_entity, kubetier);
            for (k, v) in full_entity {
                data.entry(k).or_insert(v);
            }
        }
        data.insert(
            "provenance".to_string(),
            provenance_value(campaign.entity_provenance(&entity.entity_id())),
        );
        attach_hosted_services(&mut data, &id, &hosted_services);

        entities.insert(id, data);
    }

    CampaignState {
        entities,
        relations: campaign
            .get_relations()
            .iter()
            .map(|r| {
                let mut m = HashMap::new();
                m.insert(
                    "id".to_string(),
                    Value::String(format!("{}-[{}]->{}", r.source_id, r.name, r.target_id)),
                );
                m.insert("name".to_string(), Value::String(r.name.clone()));
                m.insert("sourceId".to_string(), Value::String(r.source_id.clone()));
                m.insert("targetId".to_string(), Value::String(r.target_id.clone()));
                if let Some(ref sid) = r.session_id {
                    m.insert("sessionId".to_string(), Value::String(sid.clone()));
                }
                if r.broken {
                    m.insert("broken".to_string(), Value::Bool(true));
                }
                m.insert(
                    "provenance".to_string(),
                    provenance_value(campaign.relation_provenance(
                        &r.name,
                        &r.source_id,
                        &r.target_id,
                    )),
                );
                m
            })
            .collect(),
        bootstrap_operations: Some(bootstrap_operations(campaign)),
    }
}

fn bootstrap_operations(campaign: &Campaign) -> Vec<BootstrapOperation> {
    let mut operations = campaign
        .entities
        .values::<K8sCredential>()
        .filter_map(|credential| {
            let credential_id = credential.entity_id();
            let provenance = campaign.entity_provenance(&credential_id);
            if !provenance.contains(&campaign::KnowledgeProvenance::Operator)
                && !provenance.contains(&campaign::KnowledgeProvenance::Scenario)
            {
                return None;
            }

            let cluster_id = campaign
                .graph
                .targets_of(&credential_id, "authenticates-to")
                .first()
                .cloned()
                .cloned()?;
            let cluster = campaign
                .get_entities()
                .into_iter()
                .find(|entity| entity.entity_id() == cluster_id)?;
            let mut effects = vec![bootstrap_effect(
                credential_id.clone(),
                credential.entity_name(),
                credential.entity_kind(),
            )];
            effects.push(bootstrap_effect(
                cluster_id.clone(),
                cluster.entity_name(),
                cluster.entity_kind(),
            ));

            if let Some(namespace) = credential.default_namespace.as_deref() {
                let namespace_id = EntityId::new(format!("ns/{namespace}"));
                let is_contained = campaign
                    .graph
                    .targets_of(&cluster_id, "contains")
                    .contains(&&namespace_id);
                if is_contained {
                    if let Some(namespace_entity) = campaign
                        .get_entities()
                        .into_iter()
                        .find(|entity| entity.entity_id() == namespace_id)
                    {
                        effects.push(bootstrap_effect(
                            namespace_id,
                            namespace_entity.entity_name(),
                            namespace_entity.entity_kind(),
                        ));
                    }
                }
            }

            let detail = match credential.context_name.as_deref() {
                Some(context) => format!("{} (context: {context})", credential.entity_name()),
                None => credential.entity_name().to_string(),
            };
            Some(BootstrapOperation {
                id: format!("bootstrap:kubeconfig:{}", credential_id.0),
                name: "Read kubeconfig".to_string(),
                detail,
                effects,
            })
        })
        .collect::<Vec<_>>();
    operations.sort_by(|a, b| a.id.cmp(&b.id));
    operations
}

fn bootstrap_effect(entity_id: EntityId, entity_name: &str, entity_kind: &str) -> BootstrapEffect {
    BootstrapEffect {
        entity_id: entity_id.0,
        entity_name: entity_name.to_string(),
        entity_kind: entity_kind.to_string(),
        category: if entity_kind == "K8sCredential" {
            "credential".to_string()
        } else {
            "discovery".to_string()
        },
    }
}

pub(crate) fn campaign_to_graph(campaign: &Campaign, kubetier: &kubetier::Catalog) -> Graph {
    let entities = campaign.get_entities();
    let hosted_services = hosted_app_services(campaign);
    let hosted_listeners = hosted_listeners(campaign);
    let hosted_redirectors = hosted_redirectors(campaign);
    // AppServices, Listeners and Redirectors are rendered on the entity that
    // hosts them rather than as nodes of their own, so neither they nor their
    // hosting relations reach the graph.
    let endpoint_ids: HashSet<String> = entities
        .iter()
        .filter(|entity| {
            matches!(
                entity.entity_kind(),
                "AppService" | "Listener" | "Redirector"
            )
        })
        .map(|entity| entity.entity_id().0)
        .collect();
    let namespace_ids: HashSet<String> = entities
        .iter()
        .filter(|e| e.entity_kind() == "Namespace")
        .map(|e| e.entity_id().0)
        .collect();

    let root_node_id = entities
        .iter()
        .find(|e| e.entity_kind() == "C2")
        .map(|e| e.entity_id().0)
        .unwrap_or_default();

    // Single pass over relations: hierarchical ones become compound-node parent
    // pointers (not edges); everything else becomes a GraphEdge.
    // "manages-node" / "owns" always override (high priority);
    // "contains" only fills in if no parent has been set yet (low priority).
    let mut parent_nodes: HashMap<String, String> = HashMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    for r in campaign.get_relations() {
        if endpoint_ids.contains(&r.source_id) || endpoint_ids.contains(&r.target_id) {
            continue;
        }
        match r.name.as_str() {
            "manages-node" | "owns" => {
                parent_nodes.insert(r.target_id.clone(), r.source_id.clone());
            }
            "contains" => {
                parent_nodes
                    .entry(r.target_id.clone())
                    .or_insert_with(|| r.source_id.clone());
            }
            _ => {
                // Skip marker relations whose target is a wildcard like `all(k8s.node)`.
                // These are internal inference hints expanded by analyzers into concrete
                // edges; the wildcard target does not exist as a graph node.
                if r.target_id.starts_with("all(") {
                    continue;
                }
                edges.push(GraphEdge {
                    id: format!("{}-[{}]->{}", r.source_id, r.name, r.target_id),
                    source_id: r.source_id.clone(),
                    target_id: r.target_id.clone(),
                    name: r.name.clone(),
                    weight: if r.weight > 0.0 {
                        Some(r.weight as f64)
                    } else {
                        None
                    },
                    relation: None,
                    session_id: r.session_id.clone(),
                    // Only present when broken, so healthy edges stay lean and
                    // the frontend `edge[?broken]` selector reads absent as false.
                    broken: if r.broken { Some(true) } else { None },
                    provenance: Some(provenance_strings(campaign.relation_provenance(
                        &r.name,
                        &r.source_id,
                        &r.target_id,
                    ))),
                });
            }
        }
    }

    let reachable_pods = campaign.reachable_pods();

    let mut nodes = Vec::with_capacity(campaign.entity_count());

    for entity in entities {
        if matches!(
            entity.entity_kind(),
            "AppService" | "Listener" | "Redirector"
        ) {
            continue;
        }
        let id = entity.entity_id().0;
        let kind = entity.entity_kind().to_string();
        // Determine compound-node parent. Explicit relation-based parents take
        // precedence; namespaced resources fall back to their namespace node.
        // Cluster and Namespace nodes are top-level unless an explicit hierarchy
        // relation says otherwise. C2 is explicitly contained by OperatorHost.
        let parent = if let Some(p) = parent_nodes.get(&id) {
            Some(p.clone())
        } else {
            match &entity {
                CampaignEntityRef::Pod(_) | CampaignEntityRef::ServiceAccount(_) => entity
                    .namespace()
                    .map(|ns| format!("ns/{}", ns))
                    .filter(|ns_id| namespace_ids.contains(ns_id)),
                _ => None,
            }
        };

        let compromised = match &entity {
            CampaignEntityRef::Pod(_) => Some(reachable_pods.contains(&id)),
            CampaignEntityRef::ServiceAccount(sa) => {
                Some(sa.token.as_ref().is_some_and(|t| !t.jwt.is_empty()))
            }
            CampaignEntityRef::Node(n) => Some(n.system.access_level >= AccessLevel::Exec),
            _ => None,
        };

        let mut entity_payload = serialize_campaign_entity_map(&entity);
        if let Some(ref mut payload) = entity_payload {
            prune_entity_payload_for_ui(entity.entity_kind(), payload, kubetier);
            attach_hosted_services(payload, &id, &hosted_services);
            attach_hosted_listeners(payload, &id, &hosted_listeners);
            attach_hosted_redirectors(payload, &id, &hosted_redirectors);
        }
        nodes.push(GraphNode {
            id: id.clone(),
            entity_id: id,
            kind,
            name: entity.entity_name().to_string(),
            parent,
            access_level: None,
            compromised,
            is_running: match &entity {
                CampaignEntityRef::Pod(pod) => Some(
                    pod.is_running
                        || !matches!(pod.phase, Some(PodPhase::Succeeded | PodPhase::Failed)),
                ),
                _ => None,
            },
            entity: entity_payload,
            provenance: Some(provenance_strings(
                campaign.entity_provenance(&entity.entity_id()),
            )),
        });
    }

    Graph {
        root_node_id,
        nodes,
        edges,
    }
}

fn hosted_app_services(campaign: &Campaign) -> HashMap<String, Vec<Value>> {
    let mut endpoint_payloads = HashMap::new();
    for entity in campaign.get_entities() {
        if !matches!(entity, CampaignEntityRef::AppService(_)) {
            continue;
        }
        let id = entity.entity_id().0;
        let mut payload = serialize_campaign_entity_map(&entity).unwrap_or_default();
        payload.insert("id".to_string(), Value::String(id.clone()));
        payload.insert(
            "kind".to_string(),
            Value::String(entity.entity_kind().to_string()),
        );
        endpoint_payloads.insert(id, Value::Object(payload.into_iter().collect()));
    }

    let mut hosted: HashMap<String, Vec<Value>> = HashMap::new();
    for relation in campaign.get_relations() {
        if relation.name != "hosts-service" {
            continue;
        }
        if let Some(payload) = endpoint_payloads.get(&relation.target_id) {
            hosted
                .entry(relation.source_id)
                .or_default()
                .push(payload.clone());
        }
    }
    for services in hosted.values_mut() {
        services.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    }
    hosted
}

/// Listener payloads keyed by the C2 that holds them, via `hosts-listener`.
///
/// Mirrors [`hosted_app_services`]: the entity is real campaign state, but the
/// UI shows it on its host - here as a port badge on the C2 node.
fn hosted_listeners(campaign: &Campaign) -> HashMap<String, Vec<Value>> {
    let mut listener_payloads = HashMap::new();
    for entity in campaign.get_entities() {
        let CampaignEntityRef::Listener(listener) = entity else {
            continue;
        };
        let id = entity.entity_id().0;
        let payload = HashMap::from([
            ("id".to_string(), Value::String(id.clone())),
            ("kind".to_string(), Value::String("Listener".to_string())),
            (
                "entry".to_string(),
                Value::String(listener.entry().to_string()),
            ),
            (
                "protocol".to_string(),
                Value::String(listener.protocol.clone()),
            ),
            ("port".to_string(), Value::from(listener.port)),
        ]);
        listener_payloads.insert(id, Value::Object(payload.into_iter().collect()));
    }

    let mut hosted: HashMap<String, Vec<Value>> = HashMap::new();
    for relation in campaign.get_relations() {
        if relation.name != "hosts-listener" {
            continue;
        }
        if let Some(payload) = listener_payloads.get(&relation.target_id) {
            hosted
                .entry(relation.source_id)
                .or_default()
                .push(payload.clone());
        }
    }
    // Stable order so badges don't reshuffle between refreshes.
    for listeners in hosted.values_mut() {
        listeners.sort_by_key(|listener| listener["port"].as_u64());
    }
    hosted
}

fn attach_hosted_listeners(
    payload: &mut HashMap<String, Value>,
    entity_id: &str,
    hosted_listeners: &HashMap<String, Vec<Value>>,
) {
    let Some(listeners) = hosted_listeners.get(entity_id) else {
        return;
    };
    payload.insert("listeners".to_string(), Value::Array(listeners.clone()));
}

/// Redirector payloads keyed by the C2 whose listener they forward into.
///
/// Same treatment as [`hosted_listeners`], one hop further out: a redirector is
/// linked to its listener by `forwards-to`, and the listener to its C2 by
/// `hosts-listener`, so the badge lands on the C2 the operator is looking at.
/// A redirector whose listener has since been stopped falls back to the campaign's
/// C2 — its `labctl` tunnel is still up, so hiding it would leave the operator no
/// way to reach "Stop Redirector".
fn hosted_redirectors(campaign: &Campaign) -> HashMap<String, Vec<Value>> {
    let mut redirector_payloads = HashMap::new();
    for entity in campaign.get_entities() {
        let CampaignEntityRef::Redirector(redirector) = entity else {
            continue;
        };
        let id = entity.entity_id().0;
        let payload = HashMap::from([
            ("id".to_string(), Value::String(id.clone())),
            ("kind".to_string(), Value::String("Redirector".to_string())),
            (
                "entry".to_string(),
                Value::String(redirector.entry().to_string()),
            ),
            // What the operator reads: the tool and the hop. The playground id
            // rides along on `playId` for the rare case that two of them need
            // telling apart, but it is not the name.
            (
                "label".to_string(),
                Value::String(redirector.label().to_string()),
            ),
            ("via".to_string(), Value::String(redirector.via.clone())),
            (
                "playId".to_string(),
                Value::String(redirector.play_id.clone()),
            ),
            (
                "remotePort".to_string(),
                Value::from(redirector.remote_port),
            ),
            (
                "listenerPort".to_string(),
                Value::from(redirector.listener_port),
            ),
        ]);
        redirector_payloads.insert(id, Value::Object(payload.into_iter().collect()));
    }
    if redirector_payloads.is_empty() {
        return HashMap::new();
    }

    // listener id → the C2 that bound it.
    let mut listener_hosts: HashMap<String, String> = HashMap::new();
    for relation in campaign.get_relations() {
        if relation.name == "hosts-listener" {
            listener_hosts.insert(relation.target_id, relation.source_id);
        }
    }
    // The entity store is a HashMap, so pick the lowest id rather than the first
    // one iteration happens to yield — otherwise a campaign with more than one C2
    // would move orphaned badges between nodes on every refresh.
    let fallback_c2 = campaign
        .get_entities()
        .iter()
        .filter(|entity| entity.entity_kind() == "C2")
        .map(|entity| entity.entity_id().0)
        .min();

    let mut hosted: HashMap<String, Vec<Value>> = HashMap::new();
    let mut placed: HashSet<String> = HashSet::new();
    for relation in campaign.get_relations() {
        if relation.name != "forwards-to" {
            continue;
        }
        let Some(payload) = redirector_payloads.get(&relation.source_id) else {
            continue;
        };
        let Some(c2_id) = listener_hosts.get(&relation.target_id) else {
            continue;
        };
        hosted
            .entry(c2_id.clone())
            .or_default()
            .push(payload.clone());
        placed.insert(relation.source_id);
    }
    if let Some(fallback_c2) = fallback_c2 {
        for (id, payload) in &redirector_payloads {
            if placed.contains(id) {
                continue;
            }
            hosted
                .entry(fallback_c2.clone())
                .or_default()
                .push(payload.clone());
        }
    }
    // Stable order so badges don't reshuffle between refreshes.
    for redirectors in hosted.values_mut() {
        redirectors.sort_by_key(|redirector| {
            (
                redirector["remotePort"].as_u64(),
                redirector["id"].as_str().map(str::to_string),
            )
        });
    }
    hosted
}

fn attach_hosted_redirectors(
    payload: &mut HashMap<String, Value>,
    entity_id: &str,
    hosted_redirectors: &HashMap<String, Vec<Value>>,
) {
    let Some(redirectors) = hosted_redirectors.get(entity_id) else {
        return;
    };
    payload.insert("redirectors".to_string(), Value::Array(redirectors.clone()));
}

fn attach_hosted_services(
    payload: &mut HashMap<String, Value>,
    entity_id: &str,
    hosted_services: &HashMap<String, Vec<Value>>,
) {
    let Some(services) = hosted_services.get(entity_id) else {
        return;
    };
    payload.insert(
        "appServiceCount".to_string(),
        Value::from(services.len() as u64),
    );
    payload.insert("appServices".to_string(), Value::Array(services.clone()));
}

fn serialize_entity_map<T: serde::Serialize>(entity: &T) -> Option<HashMap<String, Value>> {
    match serde_json::to_value(entity).ok()? {
        Value::Object(map) => Some(map.into_iter().collect()),
        _ => None,
    }
}

pub(crate) fn serialize_campaign_entity_map(
    entity: &CampaignEntityRef<'_>,
) -> Option<HashMap<String, Value>> {
    match entity {
        CampaignEntityRef::OperatorHost(e) => serialize_entity_map(e),
        CampaignEntityRef::AppService(e) => serialize_entity_map(e),
        CampaignEntityRef::C2Server(e) => serialize_entity_map(e),
        CampaignEntityRef::Listener(e) => serialize_entity_map(e),
        CampaignEntityRef::Redirector(e) => serialize_entity_map(e),
        CampaignEntityRef::Cluster(e) => serialize_entity_map(e),
        CampaignEntityRef::Node(e) => serialize_entity_map(e),
        CampaignEntityRef::Namespace(e) => serialize_entity_map(e),
        CampaignEntityRef::Pod(e) => serialize_entity_map(e),
        CampaignEntityRef::ServiceAccount(e) => serialize_entity_map(e),
        CampaignEntityRef::Secret(e) => serialize_entity_map(e),
        CampaignEntityRef::ConfigMap(e) => serialize_entity_map(e),
        CampaignEntityRef::Deployment(e) => serialize_entity_map(e),
        CampaignEntityRef::Role(e) => serialize_entity_map(e),
        CampaignEntityRef::RoleBinding(e) => serialize_entity_map(e),
        CampaignEntityRef::CronJob(e) => serialize_entity_map(e),
        CampaignEntityRef::ReplicaSet(e) => serialize_entity_map(e),
        CampaignEntityRef::StatefulSet(e) => serialize_entity_map(e),
        CampaignEntityRef::DaemonSet(e) => serialize_entity_map(e),
        CampaignEntityRef::Job(e) => serialize_entity_map(e),
        CampaignEntityRef::GCPServiceAccount(e) => serialize_entity_map(e),
        CampaignEntityRef::GCPBucket(e) => serialize_entity_map(e),
        CampaignEntityRef::K8sCredential(e) => serialize_entity_map(e),
        CampaignEntityRef::UnknownSystem(e) => serialize_entity_map(e),
        CampaignEntityRef::Service(e) => serialize_entity_map(e),
        CampaignEntityRef::Ingress(e) => serialize_entity_map(e),
        CampaignEntityRef::Gateway(e) => serialize_entity_map(e),
        CampaignEntityRef::HTTPRoute(e) => serialize_entity_map(e),
    }
}

fn prune_entity_payload_for_ui(
    kind: &str,
    data: &mut HashMap<String, Value>,
    kubetier: &kubetier::Catalog,
) {
    prune_null_entries(data);

    if kind == "Pod" {
        for key in [
            "privileged",
            "host_pid",
            "host_ipc",
            "host_network",
            "read_only_root_fs",
            "automount_service_account_token",
        ] {
            if data.get(key).is_some_and(is_confidence_unknown) {
                data.remove(key);
            }
        }

        let phase_missing_or_unknown = data
            .get("phase")
            .is_none_or(|v| v.is_null() || is_unknown_enum_value(v));
        if phase_missing_or_unknown {
            data.remove("phase");
            data.remove("is_running");
        }
    }

    // Remove accessLevel when it carries no information (flattened from SystemInfo).
    // SystemInfo serializes access_level as "accessLevel" via #[serde(rename = "accessLevel")].
    if data.get("accessLevel").is_some_and(is_access_level_none) {
        data.remove("accessLevel");
    }

    if kind == "ServiceAccount" || kind == "K8sCredential" {
        let entitlements_reviewed = data
            .remove("entitlements_reviewed")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        // Rename `entitlements` → `can` and convert each permission's snake_case
        // field names to the camelCase names the frontend EntitlementInfo component expects.
        if let Some(entitlements) = data.remove("entitlements") {
            let can = rbac_permissions_to_ui(entitlements, kubetier);
            if let Value::Array(ref arr) = can {
                if !arr.is_empty() || entitlements_reviewed {
                    data.insert("can".to_string(), can);
                }
            }
        }
    }

    if kind == "Role" || kind == "ClusterRole" {
        if let Some(permissions) = data.remove("permissions") {
            data.insert(
                "permissions".to_string(),
                rbac_permissions_to_ui(permissions, kubetier),
            );
        }
    }

    if kind == "K8sCredential" {
        data.remove("token");
        data.remove("cert_data");
        data.remove("key_data");
        data.remove("ca_data");
    }

    if kind == "Redirector" {
        // Both are derived, and the details panel already shows what they are
        // built from: `label` is the entity's name, shown at the top of the box,
        // and `entry` is just `play_id` and `remote_port` glued together, both of
        // which appear as their own fields. They exist on the struct because
        // `Entity::entity_name` returns a borrow and cannot format one.
        data.remove("label");
        data.remove("entry");
    }
}

fn provenance_strings(
    origins: std::collections::BTreeSet<campaign::KnowledgeProvenance>,
) -> Vec<String> {
    origins
        .into_iter()
        .map(|origin| match origin {
            campaign::KnowledgeProvenance::Scenario => "scenario",
            campaign::KnowledgeProvenance::Operator => "operator",
            campaign::KnowledgeProvenance::Action => "action",
            campaign::KnowledgeProvenance::Inference => "inference",
        })
        .map(str::to_string)
        .collect()
}

fn provenance_value(origins: std::collections::BTreeSet<campaign::KnowledgeProvenance>) -> Value {
    Value::Array(
        provenance_strings(origins)
            .into_iter()
            .map(Value::String)
            .collect(),
    )
}

/// Convert a serialized `Vec<RbacPermission>` (snake_case keys) into the
/// camelCase shape the frontend `EntitlementInfo` component expects.
fn rbac_permissions_to_ui(value: Value, catalog: &kubetier::Catalog) -> Value {
    let Value::Array(perms) = value else {
        return value;
    };
    Value::Array(
        perms
            .into_iter()
            .map(|p| {
                let Value::Object(map) = p else { return p };
                let assessment = kubetier_assessment(&map, catalog);
                let mut out = serde_json::Map::with_capacity(map.len() + 1);
                for (k, v) in map {
                    let key = match k.as_str() {
                        "resource_type" => "resourceType",
                        "resource_name" => "resourceName",
                        "api_group" => "apiGroup",
                        "source_role" => "sourceRole",
                        "scope_kind" => "scopeKind",
                        "evaluated_namespace" => "evaluatedNamespace",
                        "scope_source" => "scopeSource",
                        other => {
                            out.insert(other.to_string(), v);
                            continue;
                        }
                    };
                    out.insert(key.to_string(), v);
                }
                if let Some(assessment) = assessment {
                    out.insert("kubetier".to_string(), assessment);
                }
                Value::Object(out)
            })
            .collect(),
    )
}

fn kubetier_assessment(
    permission: &serde_json::Map<String, Value>,
    catalog: &kubetier::Catalog,
) -> Option<Value> {
    let verb = permission.get("verb")?.as_str()?;
    let resource = permission
        .get("resource_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let resource_name = permission.get("resource_name").and_then(Value::as_str);
    let api_group = permission
        .get("api_group")
        .and_then(Value::as_str)
        .unwrap_or("");
    let scope_kind = permission
        .get("scope_kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let matches: Vec<_> = catalog
        .permissions
        .iter()
        .filter(|assessment| {
            // A literal `* *` grant maps exclusively to KubeTier's dedicated
            // wildcard assessment. SSRR may leave scope unknown, but expanding
            // that grant into every catalog row is both noisy and misleading.
            let universal_wildcard = verb == "*" && resource == "*";
            let verb_matches = if universal_wildcard {
                assessment.verb == "*"
            } else {
                verb == "*" || assessment.verb == verb
            };
            let resource_matches = if resource.is_empty() {
                resource_name.is_some_and(|url| non_resource_matches(&assessment.resource, url))
            } else if universal_wildcard {
                assessment.resource == "*"
            } else {
                resource == "*" || assessment.resource == resource
            };
            let group_matches = if universal_wildcard {
                assessment.api_group == "*"
            } else {
                resource.is_empty() || api_group == "*" || assessment.api_group == api_group
            };
            let scope_matches = universal_wildcard
                || match scope_kind {
                    "cluster" => assessment.scope == kubetier::Scope::Cluster,
                    "namespace" => assessment.scope == kubetier::Scope::Namespaced,
                    _ => true,
                };
            verb_matches && resource_matches && group_matches && scope_matches
        })
        .collect();

    if matches.is_empty() {
        return Some(serde_json::json!({
            "provider": "kubetier",
            "unassessed": true,
            "scopeUnverified": scope_kind == "unknown",
            "matches": []
        }));
    }

    let min = matches.iter().map(|entry| entry.tier).min()?;
    let max = matches.iter().map(|entry| entry.tier).max()?;
    Some(serde_json::json!({
        "provider": "kubetier",
        "tierMin": min,
        "tierMax": max,
        "scopeUnverified": scope_kind == "unknown",
        "matches": matches
    }))
}

fn non_resource_matches(catalog_resource: &str, url: &str) -> bool {
    catalog_resource
        .strip_prefix("nonResourceURLs:")
        .is_some_and(|patterns| {
            if url == "*" {
                return true;
            }
            patterns.split(',').any(|pattern| {
                let pattern = pattern.trim();
                pattern
                    .strip_suffix('*')
                    .map_or(pattern == url, |prefix| url.starts_with(prefix))
            })
        })
}

fn prune_null_entries(data: &mut HashMap<String, Value>) {
    let keys_to_remove: Vec<String> = data
        .iter()
        .filter_map(|(k, v)| if v.is_null() { Some(k.clone()) } else { None })
        .collect();
    for key in keys_to_remove {
        data.remove(&key);
    }
}

fn is_confidence_unknown(value: &Value) -> bool {
    matches!(value, Value::String(s) if s == "Unknown")
}

fn is_unknown_enum_value(value: &Value) -> bool {
    matches!(value, Value::String(s) if s == "Unknown")
}

fn is_access_level_none(value: &Value) -> bool {
    matches!(value, Value::String(s) if s == "none")
}

#[cfg(test)]
mod tests {
    use super::*;
    use campaign::{
        InitialClusterKnowledge, InitialKnowledge, InitialKubeconfigKnowledge, KnowledgeProvenance,
    };
    use ran_domain::{
        Entity, K8sCluster, K8sCredential, Listener, RbacPermission, Redirector, ServiceAccount,
    };
    use std::collections::BTreeSet;

    #[test]
    fn completed_empty_permission_review_exposes_empty_can() {
        let mut account = ServiceAccount::new("empty", "default");
        account.entitlements_reviewed = true;
        let mut data = serialize_entity_map(&account).unwrap();

        prune_entity_payload_for_ui(
            account.entity_kind(),
            &mut data,
            &kubetier::Catalog::embedded(),
        );

        assert_eq!(data.get("can"), Some(&serde_json::json!([])));
        assert!(!data.contains_key("entitlements_reviewed"));
    }

    #[test]
    fn hosted_app_services_are_attached_to_the_host_payload() {
        let services = vec![serde_json::json!({
            "id": "app-service/tcp/10.0.0.8/6379",
            "port": 6379,
            "product": "redis"
        })];
        let hosted = HashMap::from([("ns/default/pod/redis".to_string(), services.clone())]);
        let mut payload = HashMap::new();
        attach_hosted_services(&mut payload, "ns/default/pod/redis", &hosted);
        assert_eq!(payload.get("appServiceCount"), Some(&Value::from(1)));
        assert_eq!(payload.get("appServices"), Some(&Value::Array(services)));
    }

    #[test]
    fn listeners_ride_on_their_c2_instead_of_becoming_nodes() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("demo"));
        let c2_id = campaign
            .get_entities()
            .iter()
            .find(|e| e.entity_kind() == "C2")
            .map(|e| e.entity_id())
            .expect("bootstrap creates a C2");
        let listener = Listener::new(4444, "tcp");
        let listener_id = listener.entity_id();
        campaign.entities.insert_typed(listener);
        campaign.graph.insert_edge(
            &c2_id,
            &listener_id,
            cortex::edge_data_for("hosts-listener", None, None),
        );

        let graph = campaign_to_graph(&campaign, &kubetier::Catalog::embedded());

        assert!(
            !graph.nodes.iter().any(|node| node.id == listener_id.0),
            "a listener is drawn as a badge on its C2, not as a node"
        );
        assert!(
            !graph
                .edges
                .iter()
                .any(|edge| edge.target_id == listener_id.0),
            "the hosts-listener relation must not become an edge either"
        );

        let c2_node = graph
            .nodes
            .iter()
            .find(|node| node.id == c2_id.0)
            .expect("the C2 is still a node");
        let listeners = c2_node
            .entity
            .as_ref()
            .and_then(|payload| payload.get("listeners"))
            .and_then(Value::as_array)
            .expect("the C2 payload carries its listeners");
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0]["id"], Value::from(listener_id.0.as_str()));
        assert_eq!(listeners[0]["entry"], Value::from("tcp/4444"));
        assert_eq!(listeners[0]["port"], Value::from(4444));
    }

    /// Build a campaign with a listener on `tcp/4444` and a redirector forwarding
    /// `remote_port` into it, wired the way the runtime wires them. `link_listener`
    /// controls whether the `forwards-to` edge exists, which is what separates the
    /// normal path from the "listener already stopped" fallback.
    fn campaign_with_redirector(remote_port: u16, link_listener: bool) -> (Campaign, EntityId) {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("demo"));
        let c2_id = campaign
            .get_entities()
            .iter()
            .find(|e| e.entity_kind() == "C2")
            .map(|e| e.entity_id())
            .expect("bootstrap creates a C2");
        let listener = Listener::new(4444, "tcp");
        let listener_id = listener.entity_id();
        campaign.entities.insert_typed(listener);
        campaign.graph.insert_edge(
            &c2_id,
            &listener_id,
            cortex::edge_data_for("hosts-listener", None, None),
        );

        let redirector = Redirector::new("labctl", "zn1kqxk3ykpvxp5x", remote_port, 4444);
        let redirector_id = redirector.entity_id();
        campaign.entities.insert_typed(redirector);
        if link_listener {
            campaign.graph.insert_edge(
                &redirector_id,
                &listener_id,
                cortex::edge_data_for("forwards-to", None, None),
            );
        }
        (campaign, c2_id)
    }

    fn c2_redirectors(graph: &Graph, c2_id: &EntityId) -> Vec<Value> {
        graph
            .nodes
            .iter()
            .find(|node| node.id == c2_id.0)
            .expect("the C2 is still a node")
            .entity
            .as_ref()
            .and_then(|payload| payload.get("redirectors"))
            .and_then(Value::as_array)
            .expect("the C2 payload carries its redirectors")
            .clone()
    }

    #[test]
    fn redirectors_ride_on_the_c2_whose_listener_they_forward_into() {
        let (campaign, c2_id) = campaign_with_redirector(1337, true);
        let redirector_id = campaign
            .get_entities()
            .iter()
            .find(|e| e.entity_kind() == "Redirector")
            .map(|e| e.entity_id())
            .expect("the redirector was inserted");

        let graph = campaign_to_graph(&campaign, &kubetier::Catalog::embedded());

        assert!(
            !graph.nodes.iter().any(|node| node.id == redirector_id.0),
            "a redirector is drawn as a badge on its C2, not as a node"
        );
        assert!(
            !graph
                .edges
                .iter()
                .any(|edge| edge.source_id == redirector_id.0),
            "the forwards-to relation must not become an edge either"
        );

        let redirectors = c2_redirectors(&graph, &c2_id);
        assert_eq!(redirectors.len(), 1);
        assert_eq!(redirectors[0]["id"], Value::from(redirector_id.0.as_str()));
        assert_eq!(
            redirectors[0]["entry"],
            Value::from("zn1kqxk3ykpvxp5x/1337")
        );
        // What the badge and the details panel read: the tool and the hop, not
        // the playground's random id.
        assert_eq!(redirectors[0]["label"], Value::from("labctl 1337→4444"));
        assert_eq!(redirectors[0]["via"], Value::from("labctl"));
        assert_eq!(redirectors[0]["playId"], Value::from("zn1kqxk3ykpvxp5x"));
        assert_eq!(redirectors[0]["remotePort"], Value::from(1337));
        assert_eq!(redirectors[0]["listenerPort"], Value::from(4444));
    }

    /// The details panel shows the entity payload under a heading that is
    /// already the entity's name, so repeating it as a field is noise — and so
    /// is a field glued together from two others sitting next to it.
    #[test]
    fn a_redirectors_details_payload_drops_its_derived_fields() {
        let redirector = Redirector::new("labctl", "zn1kqxk3ykpvxp5x", 9000, 4444);
        let mut data = serialize_entity_map(&redirector).expect("redirector serializes");

        prune_entity_payload_for_ui(
            redirector.entity_kind(),
            &mut data,
            &kubetier::Catalog::embedded(),
        );

        assert!(!data.contains_key("label"), "label repeats the entity name");
        assert!(
            !data.contains_key("entry"),
            "entry is play_id and remote_port glued together"
        );
        // What is left is the part that cannot be derived from anything else on
        // screen: which tool, which playground, and which ports it joins.
        assert_eq!(data.get("via"), Some(&Value::from("labctl")));
        assert_eq!(data.get("play_id"), Some(&Value::from("zn1kqxk3ykpvxp5x")));
        assert_eq!(data.get("remote_port"), Some(&Value::from(9000)));
        assert_eq!(data.get("listener_port"), Some(&Value::from(4444)));
    }

    /// Pruning the details panel must not reach the badge payload, which is
    /// hand-built and does rely on `entry` and `label`.
    #[test]
    fn pruning_the_details_payload_leaves_the_badge_payload_intact() {
        let (campaign, c2_id) = campaign_with_redirector(9000, true);

        let graph = campaign_to_graph(&campaign, &kubetier::Catalog::embedded());

        let redirectors = c2_redirectors(&graph, &c2_id);
        assert_eq!(redirectors[0]["label"], Value::from("labctl 9000→4444"));
        assert_eq!(
            redirectors[0]["entry"],
            Value::from("zn1kqxk3ykpvxp5x/9000")
        );
    }

    #[test]
    fn a_redirector_whose_listener_is_gone_still_gets_a_badge() {
        // Its labctl tunnel is still up, so hiding it would leave the operator no
        // way to select it and reach "Stop Redirector".
        let (campaign, c2_id) = campaign_with_redirector(1337, false);

        let graph = campaign_to_graph(&campaign, &kubetier::Catalog::embedded());

        let redirectors = c2_redirectors(&graph, &c2_id);
        assert_eq!(redirectors.len(), 1);
        assert_eq!(redirectors[0]["remotePort"], Value::from(1337));
    }

    #[test]
    fn credential_payload_is_redacted_and_provenance_is_exposed() {
        let cluster = K8sCluster::new("demo").with_server(Some("https://demo".into()));
        let cluster_id = cluster.entity_id();
        let mut credential = K8sCredential::new("https://demo").with_name("developer");
        credential.context_name = Some("demo-context".to_string());
        credential.default_namespace = Some("default".to_string());
        credential.token = Some("super-secret".into());
        credential.key_data = Some("private-key".into());
        credential.cert_data = Some("certificate".into());
        credential.ca_data = Some("ca".into());
        credential.has_token = true;
        credential
            .entitlements
            .push(RbacPermission::new("list", "pods"));
        let credential_id = credential.entity_id();
        let origins = BTreeSet::from([KnowledgeProvenance::Scenario]);
        let campaign = Campaign::bootstrap_with_knowledge(
            "test",
            InitialKnowledge {
                clusters: vec![InitialClusterKnowledge {
                    cluster,
                    provenance: origins.clone(),
                }],
                kubeconfigs: vec![InitialKubeconfigKnowledge {
                    credential,
                    cluster_id,
                    provenance: origins,
                }],
                ..Default::default()
            },
        );

        let state = campaign_to_campaign_state(&campaign, &kubetier::Catalog::embedded());
        let payload = state.entities.get(&credential_id.0).unwrap();
        assert_eq!(
            payload.get("provenance"),
            Some(&serde_json::json!(["scenario"]))
        );
        for secret in ["token", "key_data", "cert_data", "ca_data"] {
            assert!(!payload.contains_key(secret));
        }
        let permission = payload
            .get("can")
            .and_then(Value::as_array)
            .and_then(|permissions| permissions.first())
            .and_then(Value::as_object)
            .unwrap();
        assert_eq!(permission.get("verb"), Some(&serde_json::json!("list")));
        assert_eq!(
            permission.get("resourceType"),
            Some(&serde_json::json!("pods"))
        );
        assert_eq!(
            permission.get("scopeKind"),
            Some(&serde_json::json!("unknown"))
        );
        assert_eq!(
            permission
                .get("kubetier")
                .and_then(Value::as_object)
                .and_then(|assessment| assessment.get("provider")),
            Some(&serde_json::json!("kubetier"))
        );

        let graph = campaign_to_graph(&campaign, &kubetier::Catalog::embedded());
        let node = graph
            .nodes
            .iter()
            .find(|node| node.id == credential_id.0)
            .unwrap();
        assert_eq!(
            node.provenance.as_deref(),
            Some(["scenario".to_string()].as_slice())
        );
        let entity = node.entity.as_ref().unwrap();
        assert!(!entity.contains_key("token"));
        assert!(graph.edges.iter().any(|edge| {
            edge.name == "authenticates-to"
                && edge.provenance.as_deref() == Some(["scenario".to_string()].as_slice())
        }));

        let operations = state.bootstrap_operations.unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].name, "Read kubeconfig");
        assert_eq!(operations[0].detail, "developer (context: demo-context)");
        assert_eq!(
            operations[0]
                .effects
                .iter()
                .map(|effect| effect.entity_kind.as_str())
                .collect::<Vec<_>>(),
            vec!["K8sCredential", "Cluster", "Namespace"]
        );
    }

    #[test]
    fn kubetier_lookup_respects_api_group_scope_wildcards_and_unmatched_permissions() {
        let catalog = kubetier::Catalog::embedded();
        let assessment = |permission: Value| {
            kubetier_assessment(permission.as_object().unwrap(), &catalog).unwrap()
        };

        let namespaced = assessment(serde_json::json!({
            "verb": "create",
            "resource_type": "deployments",
            "api_group": "apps",
            "scope_kind": "namespace"
        }));
        assert_eq!(namespaced["tierMin"], "T1");
        assert_eq!(namespaced["tierMax"], "T1");

        let wrong_group = assessment(serde_json::json!({
            "verb": "create",
            "resource_type": "deployments",
            "api_group": "extensions",
            "scope_kind": "namespace"
        }));
        assert_eq!(wrong_group["unassessed"], true);

        let uncertain = assessment(serde_json::json!({
            "verb": "list",
            "resource_type": "secrets",
            "api_group": "",
            "scope_kind": "unknown"
        }));
        assert_eq!(uncertain["tierMin"], "T0");
        assert_eq!(uncertain["tierMax"], "T1");
        assert_eq!(uncertain["scopeUnverified"], true);

        let wildcard = assessment(serde_json::json!({
            "verb": "*",
            "resource_type": "*",
            "api_group": "*",
            "scope_kind": "cluster"
        }));
        assert_eq!(wildcard["tierMin"], "T0");
        assert_eq!(wildcard["matches"].as_array().unwrap().len(), 1);

        let core_group_wildcard = assessment(serde_json::json!({
            "verb": "*",
            "resource_type": "*",
            "api_group": "",
            "scope_kind": "cluster"
        }));
        assert_eq!(core_group_wildcard["matches"].as_array().unwrap().len(), 1);
        assert_eq!(core_group_wildcard["matches"][0]["id"], "wildcard-all");

        let ssrr_wildcard = assessment(serde_json::json!({
            "verb": "*",
            "resource_type": "*",
            "api_group": "*",
            "scope_kind": "unknown"
        }));
        assert_eq!(ssrr_wildcard["matches"].as_array().unwrap().len(), 1);
        assert_eq!(ssrr_wildcard["matches"][0]["id"], "wildcard-all");

        let partial_wildcard = assessment(serde_json::json!({
            "verb": "*",
            "resource_type": "pods",
            "api_group": "",
            "scope_kind": "unknown"
        }));
        assert!(partial_wildcard["matches"].as_array().unwrap().len() > 1);

        let non_resource_wildcard = assessment(serde_json::json!({
            "verb": "*",
            "resource_type": "",
            "resource_name": "*",
            "api_group": "",
            "scope_kind": "cluster"
        }));
        assert!(!non_resource_wildcard["matches"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn action_discovered_credentials_do_not_create_bootstrap_operations() {
        let cluster = K8sCluster::new("external").with_server(Some("https://external".into()));
        let cluster_id = cluster.entity_id();
        let credential = K8sCredential::new("https://external").with_name("captured");
        let campaign = Campaign::bootstrap_with_knowledge(
            "test",
            InitialKnowledge {
                clusters: vec![InitialClusterKnowledge {
                    cluster,
                    provenance: BTreeSet::from([KnowledgeProvenance::Action]),
                }],
                kubeconfigs: vec![InitialKubeconfigKnowledge {
                    credential,
                    cluster_id,
                    provenance: BTreeSet::from([KnowledgeProvenance::Action]),
                }],
                ..Default::default()
            },
        );

        assert!(
            campaign_to_campaign_state(&campaign, &kubetier::Catalog::embedded())
                .bootstrap_operations
                .unwrap()
                .is_empty()
        );
    }
}
