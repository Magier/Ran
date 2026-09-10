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

/// What happened to an entity in a [`crate::FactsUpdate`].
///
/// Orthogonal to [`KnowledgeProvenance`], which records *who* told us about a
/// fact. This records *what happened to it*, which is what the operational
/// timeline needs: an action that creates a listener has not discovered one, and
/// re-emitting a known entity to carry a field update has not discovered it
/// either.
///
/// Ordering is by strength of claim, so [`FactsUpdate::merge`] can keep the
/// strongest when two sources describe the same entity.
///
/// [`FactsUpdate::merge`]: crate::FactsUpdate::merge
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FactOutcome {
    /// The campaign learned of an entity it had not seen before.
    #[default]
    Observed,
    /// The entity was already known; this update only refined its fields.
    Updated,
    /// The action brought the entity into existence. Only a creation site can
    /// claim this, and [`FactsUpdate::resolve_outcomes`] drops the claim if the
    /// entity turns out to have existed already.
    ///
    /// [`FactsUpdate::resolve_outcomes`]: crate::FactsUpdate::resolve_outcomes
    Created,
}

/// What sort of news an entity fact is, for an operator reading the timeline.
///
/// The third axis alongside [`KnowledgeProvenance`] (*who told us*) and
/// [`FactOutcome`] (*what happened to the entity*). Most facts get their
/// category from the entity kind via [`FactCategory::from_kind`], but a producer
/// that knows better — a session attaching to a host — says so outright.
///
/// Deriving this at the API edge instead left the frontend and backend each
/// holding a copy of the rule, and made `AccessGained` unreachable.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum FactCategory {
    /// Knowledge about the target: what exists, how it is configured.
    #[default]
    Discovery,
    /// Material that authenticates as somebody.
    Credential,
    /// The ability to run commands somewhere. Independent of [`FactOutcome`]:
    /// catching a shell on a host the campaign already knew is still news, even
    /// though the entity itself is only `Updated`.
    AccessGained,
}

impl FactCategory {
    /// The category implied by an entity kind, for producers with nothing more
    /// specific to say. This is the *only* definition of that mapping.
    pub fn from_kind(kind: &str) -> Self {
        match kind {
            "Secret" | "K8sCredential" => Self::Credential,
            _ => Self::Discovery,
        }
    }
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
