use std::any::{Any, TypeId};
use std::collections::HashMap;

use ran_domain::{
    C2Server, ConfigMap, CronJob, DaemonSet, Deployment, Entity, EntityId, GCPBucket,
    GCPServiceAccount, Job, K8sCluster, K8sCredential, K8sNode, K8sSecret, K8sRole, K8sRoleBinding,
    Merge, Namespace, Pod, ReplicaSet, ServiceAccount, StatefulSet, UnknownSystem,
};
use serde::de::MapAccess;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::entity_refs::CampaignEntityRef;

// ---------------------------------------------------------------------------
// EntityType bound alias
// ---------------------------------------------------------------------------

/// Collects all bounds required to store a type in [`EntityStore`].  A blanket
/// impl covers every type that satisfies them, so callers just write `T: EntityType`.
pub trait EntityType:
    Entity
    + Merge
    + Clone
    + Serialize
    + serde::de::DeserializeOwned
    + std::fmt::Debug
    + Send
    + Sync
    + 'static
{
}

impl<T> EntityType for T where
    T: Entity
        + Merge
        + Clone
        + Serialize
        + serde::de::DeserializeOwned
        + std::fmt::Debug
        + Send
        + Sync
        + 'static
{
}

// ---------------------------------------------------------------------------
// Per-type erased slot
// ---------------------------------------------------------------------------

trait ErasedSlot: std::fmt::Debug + Send + Sync {
    fn len(&self) -> usize;
    fn insert_entity(&mut self, id: EntityId, entity: &dyn Entity);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn all_refs<'a>(&'a self) -> Vec<CampaignEntityRef<'a>>;
    fn to_json(&self) -> serde_json::Value;
    fn populate_from_json(&mut self, val: serde_json::Value) -> Result<(), String>;
    fn clone_box(&self) -> Box<dyn ErasedSlot>;
}

#[derive(Debug)]
struct Slot<T: EntityType> {
    data: HashMap<EntityId, T>,
    make_ref: for<'a> fn(&'a T) -> CampaignEntityRef<'a>,
}

impl<T: EntityType> Slot<T> {
    fn new(make_ref: for<'a> fn(&'a T) -> CampaignEntityRef<'a>) -> Self {
        Self { data: HashMap::new(), make_ref }
    }
}

impl<T: EntityType> ErasedSlot for Slot<T> {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn insert_entity(&mut self, id: EntityId, entity: &dyn Entity) {
        if let Some(e) = entity.as_any().downcast_ref::<T>() {
            self.data
                .entry(id)
                .and_modify(|x| x.merge_from(e))
                .or_insert_with(|| e.clone());
        }
    }

    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }

    fn all_refs<'a>(&'a self) -> Vec<CampaignEntityRef<'a>> {
        self.data.values().map(self.make_ref).collect()
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(&self.data).unwrap_or(serde_json::Value::Null)
    }

    fn populate_from_json(&mut self, val: serde_json::Value) -> Result<(), String> {
        let map: HashMap<EntityId, T> =
            serde_json::from_value(val).map_err(|e| e.to_string())?;
        self.data.extend(map);
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn ErasedSlot> {
        Box::new(Slot { data: self.data.clone(), make_ref: self.make_ref })
    }
}

// ---------------------------------------------------------------------------
// EntityStore
// ---------------------------------------------------------------------------

/// A type-erased registry that holds one `HashMap<EntityId, T>` per entity
/// type.  Adding a new entity type only requires a single `register` call in
/// [`EntityStore::default`] and one variant in [`CampaignEntityRef`] — no
/// per-type struct fields, match arms, or boilerplate elsewhere.
#[derive(Debug)]
pub struct EntityStore {
    slots: HashMap<TypeId, Box<dyn ErasedSlot>>,
    /// Maps JSON field name → TypeId (used during deserialization).
    name_to_type: HashMap<String, TypeId>,
    /// Maps TypeId → JSON field name (used during serialization).
    type_to_name: HashMap<TypeId, &'static str>,
}

impl EntityStore {
    fn new() -> Self {
        Self {
            slots: HashMap::new(),
            name_to_type: HashMap::new(),
            type_to_name: HashMap::new(),
        }
    }

    /// Register a concrete entity type.
    ///
    /// `name` is the JSON field name produced during serialization.  Using the
    /// same names as the old `Campaign` struct fields preserves wire-format
    /// compatibility.
    pub fn register<T: EntityType>(
        &mut self,
        name: &'static str,
        make_ref: for<'a> fn(&'a T) -> CampaignEntityRef<'a>,
    ) {
        let tid = TypeId::of::<T>();
        self.slots.insert(tid, Box::new(Slot::<T>::new(make_ref)));
        self.name_to_type.insert(name.to_string(), tid);
        self.type_to_name.insert(tid, name);
    }

    // --- Typed access -------------------------------------------------------

    /// Read-only view of the `HashMap<EntityId, T>` for type `T`.
    ///
    /// # Panics
    /// Panics if `T` was not registered.
    pub fn get<T: EntityType>(&self) -> &HashMap<EntityId, T> {
        let tid = TypeId::of::<T>();
        let slot = self
            .slots
            .get(&tid)
            .unwrap_or_else(|| panic!("EntityStore: {} not registered", std::any::type_name::<T>()));
        &slot
            .as_any()
            .downcast_ref::<Slot<T>>()
            .expect("EntityStore: internal slot type mismatch")
            .data
    }

    /// Mutable view of the `HashMap<EntityId, T>` for type `T`.
    ///
    /// # Panics
    /// Panics if `T` was not registered.
    pub fn get_mut<T: EntityType>(&mut self) -> &mut HashMap<EntityId, T> {
        let tid = TypeId::of::<T>();
        let slot = self
            .slots
            .get_mut(&tid)
            .unwrap_or_else(|| panic!("EntityStore: {} not registered", std::any::type_name::<T>()));
        &mut slot
            .as_any_mut()
            .downcast_mut::<Slot<T>>()
            .expect("EntityStore: internal slot type mismatch")
            .data
    }

    // --- Convenience helpers ------------------------------------------------

    /// Insert `entity` keyed by its own `entity_id()`, merging via
    /// [`Merge::merge_from`] when an entry already exists.
    pub fn insert_typed<T: EntityType>(&mut self, entity: T) {
        let id = entity.entity_id();
        self.get_mut::<T>()
            .entry(id)
            .and_modify(|x| x.merge_from(&entity))
            .or_insert(entity);
    }

    /// Type-erased insert from `&dyn Entity`.  Dispatches to the correct slot
    /// via `TypeId`; silently ignores types that were not registered.
    pub fn insert_entity(&mut self, entity: &dyn Entity) {
        let tid = entity.as_any().type_id();
        let id = entity.entity_id();
        if let Some(slot) = self.slots.get_mut(&tid) {
            slot.insert_entity(id, entity);
        }
    }

    pub fn find<T: EntityType>(&self, id: &EntityId) -> Option<&T> {
        self.get::<T>().get(id)
    }

    pub fn find_mut<T: EntityType>(&mut self, id: &EntityId) -> Option<&mut T> {
        self.get_mut::<T>().get_mut(id)
    }

    pub fn contains<T: EntityType>(&self, id: &EntityId) -> bool {
        self.get::<T>().contains_key(id)
    }

    pub fn values<T: EntityType>(&self) -> std::collections::hash_map::Values<'_, EntityId, T> {
        self.get::<T>().values()
    }

    // --- Cross-type operations ----------------------------------------------

    pub fn entity_count(&self) -> usize {
        self.slots.values().map(|s| s.len()).sum()
    }

    /// Returns a `CampaignEntityRef` for every entity across all registered types.
    pub fn all_entities<'a>(&'a self) -> Vec<CampaignEntityRef<'a>> {
        let mut result = Vec::with_capacity(self.entity_count());
        for slot in self.slots.values() {
            result.extend(slot.all_refs());
        }
        result
    }
}

impl Clone for EntityStore {
    fn clone(&self) -> Self {
        Self {
            slots: self.slots.iter().map(|(&k, v)| (k, v.clone_box())).collect(),
            name_to_type: self.name_to_type.clone(),
            type_to_name: self.type_to_name.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Default — the single place to register all known entity types
// ---------------------------------------------------------------------------

impl Default for EntityStore {
    /// Creates a store pre-registered for all entity types the campaign knows about.
    ///
    /// **To add a new entity type:** add one `s.register::<NewType>(...)` call
    /// here and one variant to [`CampaignEntityRef`].  No other files need to change.
    fn default() -> Self {
        let mut s = Self::new();
        s.register::<C2Server>("c2_servers", |t| CampaignEntityRef::C2Server(t));
        s.register::<K8sCluster>("clusters", |t| CampaignEntityRef::Cluster(t));
        s.register::<K8sNode>("nodes", |t| CampaignEntityRef::Node(t));
        s.register::<Namespace>("namespaces", |t| CampaignEntityRef::Namespace(t));
        s.register::<Pod>("pods", |t| CampaignEntityRef::Pod(t));
        s.register::<ServiceAccount>("service_accounts", |t| CampaignEntityRef::ServiceAccount(t));
        s.register::<K8sSecret>("secrets", |t| CampaignEntityRef::Secret(t));
        s.register::<ConfigMap>("config_maps", |t| CampaignEntityRef::ConfigMap(t));
        s.register::<Deployment>("deployments", |t| CampaignEntityRef::Deployment(t));
        s.register::<K8sRole>("roles", |t| CampaignEntityRef::Role(t));
        s.register::<K8sRoleBinding>("role_bindings", |t| CampaignEntityRef::RoleBinding(t));
        s.register::<CronJob>("cron_jobs", |t| CampaignEntityRef::CronJob(t));
        s.register::<ReplicaSet>("replica_sets", |t| CampaignEntityRef::ReplicaSet(t));
        s.register::<StatefulSet>("stateful_sets", |t| CampaignEntityRef::StatefulSet(t));
        s.register::<DaemonSet>("daemon_sets", |t| CampaignEntityRef::DaemonSet(t));
        s.register::<Job>("jobs", |t| CampaignEntityRef::Job(t));
        s.register::<GCPServiceAccount>("gcp_service_accounts", |t| CampaignEntityRef::GCPServiceAccount(t));
        s.register::<GCPBucket>("gcp_buckets", |t| CampaignEntityRef::GCPBucket(t));
        s.register::<K8sCredential>("k8s_credentials", |t| CampaignEntityRef::K8sCredential(t));
        s.register::<UnknownSystem>("unknown_systems", |t| CampaignEntityRef::UnknownSystem(t));
        s
    }
}

// ---------------------------------------------------------------------------
// Serde — serializes each slot under its registered field name
// ---------------------------------------------------------------------------
//
// The JSON wire format is identical to the previous `Campaign` struct layout
// (flat named fields), so existing serialized state remains compatible.

impl Serialize for EntityStore {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.slots.len()))?;
        // Sort by field name for deterministic output.
        let mut pairs: Vec<(&'static str, TypeId)> = self
            .type_to_name
            .iter()
            .map(|(&tid, &name)| (name, tid))
            .collect();
        pairs.sort_by_key(|(name, _)| *name);

        for (name, tid) in &pairs {
            if let Some(slot) = self.slots.get(tid) {
                map.serialize_entry(name, &slot.to_json())?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for EntityStore {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = EntityStore;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a map of entity collections")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut store = EntityStore::default();
                while let Some(key) = map.next_key::<String>()? {
                    let val: serde_json::Value = map.next_value()?;
                    if let Some(tid) = store.name_to_type.get(&key).copied() {
                        if let Some(slot) = store.slots.get_mut(&tid) {
                            slot.populate_from_json(val)
                                .map_err(serde::de::Error::custom)?;
                        }
                    }
                    // Unknown keys are silently ignored for forward compatibility.
                }
                Ok(store)
            }
        }

        d.deserialize_map(Visitor)
    }
}
