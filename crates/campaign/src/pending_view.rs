use std::borrow::Cow;

use ran_domain::{EntityId, Namespace, RelationSummary};

use crate::campaign::EntityType;
use crate::{Campaign, FactsUpdate};

/// A read-only view over committed campaign state plus a pending `FactsUpdate`.
///
/// Every `InferenceRule::infer` receives `(&Campaign, &FactsUpdate)`. Wrapping
/// them in a `PendingView` at the top of the function replaces the repeated
/// "find in campaign, then scan update, then fall back to a stub" pattern that
/// would otherwise be copy-pasted across every rule.
///
/// ```ignore
/// fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
///     let view = PendingView::new(campaign, update);
///     let pods = view.collect::<Pod>();
///     let (ns_id, new_ns) = view.ensure_namespace("default");
///     // ...
/// }
/// ```
pub struct PendingView<'a> {
    campaign: &'a Campaign,
    update: &'a FactsUpdate,
}

impl<'a> PendingView<'a> {
    pub fn new(campaign: &'a Campaign, update: &'a FactsUpdate) -> Self {
        Self { campaign, update }
    }

    /// Find an entity by ID, checking committed campaign state first then the
    /// pending update.
    ///
    /// Returns `Cow::Borrowed` when found in committed state (zero-copy) and
    /// `Cow::Owned` when found only in the pending update (one clone required).
    pub fn find<T: EntityType>(&self, id: &EntityId) -> Option<Cow<'a, T>> {
        if let Some(t) = self.campaign.entities.find::<T>(id) {
            return Some(Cow::Borrowed(t));
        }
        self.update.new_entities.iter().find_map(|e| {
            e.as_any()
                .downcast_ref::<T>()
                .filter(|x| x.entity_id() == *id)
                .cloned()
                .map(Cow::Owned)
        })
    }

    /// Return `true` if an entity with this ID exists in committed state or the
    /// pending update.
    ///
    /// The update check is entity-ID-only (not type-filtered), consistent with
    /// the convention used throughout the analyzers before this helper existed.
    pub fn contains<T: EntityType>(&self, id: &EntityId) -> bool {
        self.campaign.entities.contains::<T>(id)
            || self
                .update
                .new_entities
                .iter()
                .any(|e| e.entity_id() == *id)
    }

    /// Return the entity for `id` if it exists, or call `make_stub` to produce
    /// a fallback value.
    pub fn find_or_stub<T: EntityType>(&self, id: &EntityId, make_stub: impl FnOnce() -> T) -> T {
        self.find::<T>(id)
            .map(|c| c.into_owned())
            .unwrap_or_else(make_stub)
    }

    /// Collect all entities of type `T` from both committed state and the
    /// pending update, with the pending version winning when the same ID appears
    /// in both.
    pub fn collect<T: EntityType>(&self) -> Vec<T> {
        let mut items: Vec<T> = self.campaign.entities.values::<T>().cloned().collect();
        for entity in &self.update.new_entities {
            if let Some(item) = entity.as_any().downcast_ref::<T>() {
                let id = item.entity_id();
                match items.iter_mut().find(|x| x.entity_id() == id) {
                    Some(existing) => *existing = item.clone(),
                    None => items.push(item.clone()),
                }
            }
        }
        items
    }

    /// Resolve a Kubernetes namespace by name.
    ///
    /// Returns the canonical `EntityId` (`ns/<name>`) and, if the namespace is
    /// not yet present in committed campaign state, a freshly created `Namespace`
    /// entity that the caller should emit.  When the namespace is already
    /// committed, the second element is `None` (no emit needed).
    ///
    /// If the namespace was added earlier in this same update batch by another
    /// rule, it is still re-emitted here — `FactsUpdate::merge` deduplicates by
    /// entity ID so the double-emit is harmless.
    ///
    /// # Usage
    ///
    /// ```ignore
    /// let (ns_id, new_ns) = view.ensure_namespace(ns_name);
    /// if let Some(ns) = new_ns {
    ///     inferred.new_entities.push(Box::new(ns));
    /// }
    /// inferred.new_relations.push(Box::new(Contains::new(ns_id.0.clone(), child_id)));
    /// ```
    pub fn ensure_namespace(&self, ns_name: &str) -> (EntityId, Option<Namespace>) {
        let ns_id = EntityId::new(format!("ns/{}", ns_name));
        if self.campaign.entities.contains::<Namespace>(&ns_id) {
            (ns_id, None)
        } else {
            (ns_id, Some(Namespace::new(ns_name)))
        }
    }

    /// All relations from committed campaign state plus those pending in this
    /// update, deduplicated by `(name, source_id, target_id)`.
    pub fn relations(&self) -> Vec<RelationSummary> {
        let mut rels = self.campaign.graph.to_relation_summaries();
        for rel in &self.update.new_relations {
            let summary = RelationSummary::from_relation(rel.as_ref());
            let exists = rels.iter().any(|r| {
                r.name == summary.name
                    && r.source_id == summary.source_id
                    && r.target_id == summary.target_id
            });
            if !exists {
                rels.push(summary);
            }
        }
        rels
    }
}
