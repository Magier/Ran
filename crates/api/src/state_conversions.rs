use std::collections::{HashMap, HashSet};

use campaign::{Campaign, CampaignEntityRef};
use serde_json::Value;

use crate::{CampaignState, Graph, GraphEdge, GraphNode};

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
            data.insert("namespace".to_string(), Value::String(namespace.to_string()));
        }

        if let Some(mut full_entity) = serialize_campaign_entity_map(&entity) {
            prune_entity_payload_for_ui(entity.entity_kind(), &mut full_entity);
            for (k, v) in full_entity {
                data.entry(k).or_insert(v);
            }
            add_ui_system_field_aliases(&mut data);
        }

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
                m
            })
            .collect(),
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
                edges.push(GraphEdge {
                    id: format!("{}-[{}]->{}", r.source_id, r.name, r.target_id),
                    source_id: r.source_id.clone(),
                    target_id: r.target_id.clone(),
                    name: r.name.clone(),
                    weight: None,
                    relation: None,
                });
            }
        }
    }

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

        nodes.push(GraphNode {
            id: id.clone(),
            entity_id: id,
            kind,
            name: entity.entity_name().to_string(),
            parent,
            access_level: None,
            compromised: None,
            is_running: None,
            entity: serialize_campaign_entity_map(&entity),
        });
    }

    Graph { root_node_id, nodes, edges }
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
    }
}

fn add_ui_system_field_aliases(data: &mut HashMap<String, Value>) {
    // SystemInfo fields are flattened directly into the entity map.
    // Create camelCase aliases for the fields the frontend expects.
    if let Some(env_vars) = data.get("env_vars").cloned().filter(|v| !v.is_null()) {
        data.insert("envVars".to_string(), env_vars);
    }
    if let Some(access_level) = data.get("access_level").cloned().filter(|v| !is_access_level_none(v)) {
        data.insert("accessLevel".to_string(), access_level);
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

    // Remove access_level when it carries no information (flattened from SystemInfo).
    if data.get("access_level").is_some_and(is_access_level_none) {
        data.remove("access_level");
    }

    if kind == "ServiceAccount" {
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
                        "api_group"     => "apiGroup",
                        "source_role"   => "sourceRole",
                        other           => {
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
