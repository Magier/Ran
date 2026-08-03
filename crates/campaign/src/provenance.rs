use std::collections::{BTreeSet, HashMap};

use ran_domain::{EntityId, Relation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeProvenance {
    Scenario,
    Operator,
    Action,
    Inference,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelationProvenanceKey(String);

impl RelationProvenanceKey {
    pub fn new(
        name: impl Into<String>,
        source_id: impl Into<String>,
        target_id: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let source_id = EntityId::new(source_id);
        let target_id = EntityId::new(target_id);
        Self(format!(
            "{}\u{1f}{}\u{1f}{}",
            name, source_id.0, target_id.0
        ))
    }

    pub fn from_relation(relation: &dyn Relation) -> Self {
        Self::new(
            relation.relation_name(),
            relation.source_id().0.clone(),
            relation.target_id().0.clone(),
        )
    }

    fn parts(&self) -> Option<(&str, &str, &str)> {
        let mut parts = self.0.splitn(3, '\u{1f}');
        Some((parts.next()?, parts.next()?, parts.next()?))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnowledgeProvenanceStore {
    #[serde(default)]
    pub entities: HashMap<EntityId, BTreeSet<KnowledgeProvenance>>,
    #[serde(default)]
    pub relations: HashMap<RelationProvenanceKey, BTreeSet<KnowledgeProvenance>>,
}

impl KnowledgeProvenanceStore {
    pub fn add_entity(&mut self, id: EntityId, provenance: KnowledgeProvenance) {
        self.entities.entry(id).or_default().insert(provenance);
    }

    pub fn add_relation(&mut self, key: RelationProvenanceKey, provenance: KnowledgeProvenance) {
        self.relations.entry(key).or_default().insert(provenance);
    }

    pub fn entity(&self, id: &EntityId) -> BTreeSet<KnowledgeProvenance> {
        self.entities.get(id).cloned().unwrap_or_default()
    }

    pub fn relation(&self, key: &RelationProvenanceKey) -> BTreeSet<KnowledgeProvenance> {
        self.relations.get(key).cloned().unwrap_or_default()
    }

    pub fn merge_entity(&mut self, stale: &EntityId, preferred: &EntityId) {
        if let Some(origins) = self.entities.remove(stale) {
            self.entities
                .entry(preferred.clone())
                .or_default()
                .extend(origins);
        }

        let mut rewritten = HashMap::new();
        for (key, origins) in std::mem::take(&mut self.relations) {
            let Some((name, source, target)) = key.parts() else {
                rewritten.entry(key).or_insert(origins);
                continue;
            };
            let source = if source == stale.0 {
                preferred.0.as_str()
            } else {
                source
            };
            let target = if target == stale.0 {
                preferred.0.as_str()
            } else {
                target
            };
            rewritten
                .entry(RelationProvenanceKey::new(name, source, target))
                .or_insert_with(BTreeSet::new)
                .extend(origins);
        }
        self.relations = rewritten;
    }
}
