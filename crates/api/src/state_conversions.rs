use std::collections::{HashMap, HashSet};

use campaign::{Campaign, CampaignEntityRef};
use ran_domain::{AccessLevel, Entity, EntityId, K8sCredential, PodPhase};
use serde_json::Value;

use crate::{BootstrapEffect, BootstrapOperation, CampaignState, Graph, GraphEdge, GraphNode};

pub(crate) fn campaign_to_campaign_state(campaign: &Campaign) -> CampaignState {
    let mut entities = HashMap::new();

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
            prune_entity_payload_for_ui(entity.entity_kind(), &mut full_entity);
            for (k, v) in full_entity {
                data.entry(k).or_insert(v);
            }
        }
        data.insert(
            "provenance".to_string(),
            provenance_value(campaign.entity_provenance(&entity.entity_id())),
        );

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

pub(crate) fn campaign_to_graph(campaign: &Campaign) -> Graph {
    let entities = campaign.get_entities();
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
        let id = entity.entity_id().0;
        let kind = entity.entity_kind().to_string();
        // Determine compound-node parent. Explicit relation-based parents take
        // precedence; namespaced resources fall back to their namespace node.
        // C2, Cluster, and Namespace nodes are top-level to avoid nested compound
        // nodes, which fcose cannot handle and will crash with invalid array length.
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
            prune_entity_payload_for_ui(entity.entity_kind(), payload);
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
        CampaignEntityRef::C2Server(e) => serialize_entity_map(e),
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

fn prune_entity_payload_for_ui(kind: &str, data: &mut HashMap<String, Value>) {
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
        // Rename `entitlements` → `can` and convert each permission's snake_case
        // field names to the camelCase names the frontend EntitlementInfo component expects.
        if let Some(entitlements) = data.remove("entitlements") {
            let can = rbac_permissions_to_ui(entitlements);
            if let Value::Array(ref arr) = can {
                if !arr.is_empty() {
                    data.insert("can".to_string(), can);
                }
            }
        }
    }

    if kind == "K8sCredential" {
        data.remove("token");
        data.remove("cert_data");
        data.remove("key_data");
        data.remove("ca_data");
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
fn rbac_permissions_to_ui(value: Value) -> Value {
    let Value::Array(perms) = value else {
        return value;
    };
    Value::Array(
        perms
            .into_iter()
            .map(|p| {
                let Value::Object(map) = p else { return p };
                let mut out = serde_json::Map::with_capacity(map.len());
                for (k, v) in map {
                    let key = match k.as_str() {
                        "resource_type" => "resourceType",
                        "resource_name" => "resourceName",
                        "api_group" => "apiGroup",
                        "source_role" => "sourceRole",
                        other => {
                            out.insert(other.to_string(), v);
                            continue;
                        }
                    };
                    out.insert(key.to_string(), v);
                }
                Value::Object(out)
            })
            .collect(),
    )
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
    use ran_domain::{Entity, K8sCluster, K8sCredential, RbacPermission};
    use std::collections::BTreeSet;

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
            },
        );

        let state = campaign_to_campaign_state(&campaign);
        let payload = state.entities.get(&credential_id.0).unwrap();
        assert_eq!(
            payload.get("provenance"),
            Some(&serde_json::json!(["scenario"]))
        );
        for secret in ["token", "key_data", "cert_data", "ca_data"] {
            assert!(!payload.contains_key(secret));
        }
        assert_eq!(
            payload.get("can"),
            Some(&serde_json::json!([{
                "verb": "list",
                "resourceType": "pods",
                "resourceName": null,
                "apiGroup": null,
                "scope": null,
                "sourceRole": null
            }]))
        );

        let graph = campaign_to_graph(&campaign);
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
            },
        );

        assert!(campaign_to_campaign_state(&campaign)
            .bootstrap_operations
            .unwrap()
            .is_empty());
    }
}
