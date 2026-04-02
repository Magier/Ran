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
