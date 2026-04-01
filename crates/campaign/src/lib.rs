use std::collections::HashMap;

use armory::{Armory, Procedure, Ttp};
use c2::{ExecTtp, TtpExecuted};
use ran_domain::{
    C2Server, Entity, EntityId, K8sCluster, K8sNode, Namespace, Pod, RelationSummary,
    ServiceAccount,
};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod analyzers;
pub mod effects;
pub mod rules;
pub mod runtime;
pub use effects::FactsUpdate;
pub use rules::{default_rules, run_rules_fixpoint, InferenceRule, RuleTrigger};
pub use runtime::{spawn_c2_event_processor, CampaignEvent, CampaignEventBus, EntitySummary};
use effects::{ground_template, parse_effect};
use rules::default_rules as make_rules;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteActionRequest {
    pub action_id: String,
    pub exec_system_id: Option<String>,
    pub target_id: String,
    pub procedure_id: Option<String>,
    pub args: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutedActionEvent {
    pub id: String,
    pub cmd_id: String,
    pub ttp: Ttp,
    pub args: HashMap<String, String>,
    pub exec_system_id: String,
    pub success: bool,
    pub fail_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteActionResult {
    pub cmd_id: String,
    pub event: ExecutedActionEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecuteActionError {
    InvalidInput(String),
    NotFound(String),
}

pub enum CampaignEntityRef<'a> {
    C2Server(&'a C2Server),
    Cluster(&'a K8sCluster),
    Node(&'a K8sNode),
    Namespace(&'a Namespace),
    Pod(&'a Pod),
    ServiceAccount(&'a ServiceAccount),
}

impl<'a> CampaignEntityRef<'a> {
    pub fn entity_id(&self) -> EntityId {
        match self {
            CampaignEntityRef::C2Server(e) => e.entity_id(),
            CampaignEntityRef::Cluster(e) => e.entity_id(),
            CampaignEntityRef::Node(e) => e.entity_id(),
            CampaignEntityRef::Namespace(e) => e.entity_id(),
            CampaignEntityRef::Pod(e) => e.entity_id(),
            CampaignEntityRef::ServiceAccount(e) => e.entity_id(),
        }
    }

    pub fn entity_name(&self) -> &str {
        match self {
            CampaignEntityRef::C2Server(e) => e.entity_name(),
            CampaignEntityRef::Cluster(e) => e.entity_name(),
            CampaignEntityRef::Node(e) => e.entity_name(),
            CampaignEntityRef::Namespace(e) => e.entity_name(),
            CampaignEntityRef::Pod(e) => e.entity_name(),
            CampaignEntityRef::ServiceAccount(e) => e.entity_name(),
        }
    }

    pub fn entity_kind(&self) -> &str {
        match self {
            CampaignEntityRef::C2Server(e) => e.entity_kind(),
            CampaignEntityRef::Cluster(e) => e.entity_kind(),
            CampaignEntityRef::Node(e) => e.entity_kind(),
            CampaignEntityRef::Namespace(e) => e.entity_kind(),
            CampaignEntityRef::Pod(e) => e.entity_kind(),
            CampaignEntityRef::ServiceAccount(e) => e.entity_kind(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        match self {
            CampaignEntityRef::Pod(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::ServiceAccount(e) => e.meta.namespace.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub c2_servers: HashMap<EntityId, C2Server>,
    pub clusters: HashMap<EntityId, K8sCluster>,
    pub nodes: HashMap<EntityId, K8sNode>,
    pub namespaces: HashMap<EntityId, Namespace>,
    pub pods: HashMap<EntityId, Pod>,
    pub service_accounts: HashMap<EntityId, ServiceAccount>,
    pub relations: Vec<RelationSummary>,
}

impl Campaign {
    pub fn bootstrap(ran_name: impl Into<String>, target_cluster: K8sCluster) -> Self {
        let mut c2_servers = HashMap::new();
        let mut clusters = HashMap::new();

        let c2 = C2Server::new(ran_name.into());
        c2_servers.insert(c2.entity_id(), c2);

        clusters.insert(target_cluster.entity_id(), target_cluster);

        Campaign {
            c2_servers,
            clusters,
            nodes: HashMap::new(),
            namespaces: HashMap::new(),
            pods: HashMap::new(),
            service_accounts: HashMap::new(),
            relations: Vec::new(),
        }
    }

    pub fn entity_count(&self) -> usize {
        self.c2_servers.len()
            + self.clusters.len()
            + self.nodes.len()
            + self.namespaces.len()
            + self.pods.len()
            + self.service_accounts.len()
    }

    pub fn get_entities(&self) -> Vec<CampaignEntityRef<'_>> {
        let mut entities = Vec::with_capacity(self.entity_count());

        entities.extend(self.c2_servers.values().map(CampaignEntityRef::C2Server));
        entities.extend(self.clusters.values().map(CampaignEntityRef::Cluster));
        entities.extend(self.nodes.values().map(CampaignEntityRef::Node));
        entities.extend(self.namespaces.values().map(CampaignEntityRef::Namespace));
        entities.extend(self.pods.values().map(CampaignEntityRef::Pod));
        entities.extend(
            self.service_accounts
                .values()
                .map(CampaignEntityRef::ServiceAccount),
        );

        entities
    }

    pub fn get_relations(&self) -> &[RelationSummary] {
        &self.relations
    }

    pub fn prepare_action(
        &mut self,
        request: ExecuteActionRequest,
        armory: &Armory,
    ) -> Result<ExecTtp, ExecuteActionError> {
        if request.action_id.trim().is_empty() || request.target_id.trim().is_empty() {
            return Err(ExecuteActionError::InvalidInput(
                "actionId and targetId are required".to_string(),
            ));
        }

        let target_exists = self
            .get_entities()
            .into_iter()
            .any(|entity| entity.entity_id().0 == request.target_id);
        if !target_exists {
            return Err(ExecuteActionError::NotFound(format!(
                "failed to get target entity: {}",
                request.target_id
            )));
        }

        let mut ttp = armory
            .get_ttp(&request.action_id)
            .cloned()
            .ok_or_else(|| {
                ExecuteActionError::NotFound(format!(
                    "No TTP with ID '{}' found",
                    request.action_id
                ))
            })?;

        let mut args = request.args;
        for p in &ttp.params {
            if !args.contains_key(&p.name) && !p.default.is_empty() {
                args.insert(p.name.clone(), p.default.clone());
            }
        }

        let mut procedure = self.select_procedure(&ttp, request.procedure_id.as_deref())?;
        procedure.command = ground_template(&procedure.command, &args);

        for effect in &mut ttp.effects {
            *effect = ground_template(effect, &args);
        }

        Ok(ExecTtp {
            id: generate_cmd_id(),
            ttp,
            procedure,
            args,
            target_id: request.target_id,
            exec_system_id: request.exec_system_id.unwrap_or_default(),
        })
    }

    pub fn on_ttp_executed(
        &mut self,
        cmd: &ExecTtp,
        event: &TtpExecuted,
    ) -> Result<FactsUpdate, ExecuteActionError> {
        let mut updates = FactsUpdate::default();

        if !event.success {
            return Ok(updates);
        }

        for effect in &cmd.ttp.effects {
            let parsed = parse_effect(effect, &cmd.args).map_err(ExecuteActionError::InvalidInput)?;
            updates.merge(parsed);
        }

        let rules = make_rules();
        updates = run_rules_fixpoint(self, &rules, updates);

        self.apply_facts(&updates);
        Ok(updates)
    }

    fn apply_facts(&mut self, updates: &FactsUpdate) {
        for entity in &updates.new_entities {
            self.insert_entity(entity.as_ref());
        }

        for rel in &updates.new_relations {
            let summary = RelationSummary::from_relation(rel.as_ref());

            // Invariant: a pod can run on only one node at a time.
            // If another runs-on arrives, reconcile by preferring a concrete
            // node name over placeholders and rewrite stale node references.
            if summary.name == "runs-on" {
                self.apply_runs_on_with_invariant(summary);
                continue;
            }

            let exists = self.relations.iter().any(|r| {
                r.name == summary.name
                    && r.source_id == summary.source_id
                    && r.target_id == summary.target_id
            });
            if !exists {
                self.relations.push(summary);
            }
        }
    }

    fn apply_runs_on_with_invariant(&mut self, incoming: RelationSummary) {
        let existing_idx = self
            .relations
            .iter()
            .position(|r| r.name == "runs-on" && r.source_id == incoming.source_id);

        let Some(idx) = existing_idx else {
            self.relations.push(incoming);
            return;
        };

        let existing = self.relations[idx].clone();
        if existing.target_id == incoming.target_id {
            return;
        }

        let preferred = choose_preferred_node_id(&existing.target_id, &incoming.target_id);
        let stale = if preferred == existing.target_id {
            incoming.target_id
        } else {
            existing.target_id
        };

        self.relations[idx].target_id = preferred.clone();
        self.rewrite_relation_entity_id(&stale, &preferred);
        self.merge_node_entities(&preferred, &stale);
    }

    fn rewrite_relation_entity_id(&mut self, stale_id: &str, preferred_id: &str) {
        for rel in &mut self.relations {
            if rel.source_id == stale_id {
                rel.source_id = preferred_id.to_string();
            }
            if rel.target_id == stale_id {
                rel.target_id = preferred_id.to_string();
            }
        }

        // Deduplicate summaries that became identical after rewrite.
        let mut deduped = Vec::with_capacity(self.relations.len());
        for rel in self.relations.drain(..) {
            let exists = deduped.iter().any(|r: &RelationSummary| {
                r.name == rel.name && r.source_id == rel.source_id && r.target_id == rel.target_id
            });
            if !exists {
                deduped.push(rel);
            }
        }
        self.relations = deduped;
    }

    fn merge_node_entities(&mut self, preferred_id: &str, stale_id: &str) {
        if preferred_id == stale_id {
            return;
        }

        let preferred = EntityId::new(preferred_id);
        let stale = EntityId::new(stale_id);

        if !self.nodes.contains_key(&preferred) {
            if let Some(stale_node) = self.nodes.get(&stale).cloned() {
                self.nodes.insert(preferred.clone(), stale_node);
            }
        }

        self.nodes.remove(&stale);
    }

    fn insert_entity(&mut self, entity: &dyn Entity) {
        let any = entity.as_any();
        if let Some(e) = any.downcast_ref::<Pod>() {
            self.pods.insert(e.entity_id(), e.clone());
        } else if let Some(e) = any.downcast_ref::<Namespace>() {
            self.namespaces.insert(e.entity_id(), e.clone());
        } else if let Some(e) = any.downcast_ref::<K8sCluster>() {
            self.clusters.insert(e.entity_id(), e.clone());
        } else if let Some(e) = any.downcast_ref::<K8sNode>() {
            self.nodes.insert(e.entity_id(), e.clone());
        } else if let Some(e) = any.downcast_ref::<C2Server>() {
            self.c2_servers.insert(e.entity_id(), e.clone());
        } else if let Some(e) = any.downcast_ref::<ServiceAccount>() {
            self.service_accounts.insert(e.entity_id(), e.clone());
        }
    }

    fn select_procedure(
        &self,
        ttp: &Ttp,
        procedure_id: Option<&str>,
    ) -> Result<Procedure, ExecuteActionError> {
        if let Some(proc_id) = procedure_id {
            return ttp
                .procedures
                .iter()
                .find(|p| p.id == proc_id)
                .cloned()
                .ok_or_else(|| {
                    ExecuteActionError::InvalidInput(format!(
                        "procedure '{}' not found for action '{}'",
                        proc_id, ttp.id
                    ))
                });
        }

        ttp.procedures.first().cloned().ok_or_else(|| {
            ExecuteActionError::InvalidInput(format!("No procedure found for action '{}'", ttp.id))
        })
    }

    pub fn execute_action(
        &mut self,
        request: ExecuteActionRequest,
        armory: &Armory,
    ) -> Result<ExecuteActionResult, ExecuteActionError> {
        let exec = self.prepare_action(request, armory)?;
        Ok(ExecuteActionResult {
            cmd_id: exec.id.clone(),
            event: ExecutedActionEvent {
                id: exec.id.clone(),
                cmd_id: exec.id,
                ttp: exec.ttp,
                args: exec.args,
                exec_system_id: exec.exec_system_id,
                success: true,
                fail_reason: String::new(),
            },
        })
    }
}

fn choose_preferred_node_id(a: &str, b: &str) -> String {
    let a_unknown = is_placeholder_node_id(a);
    let b_unknown = is_placeholder_node_id(b);

    match (a_unknown, b_unknown) {
        (true, false) => b.to_string(),
        (false, true) => a.to_string(),
        _ => a.to_string(),
    }
}

fn is_placeholder_node_id(node_id: &str) -> bool {
    match node_id.strip_prefix("node/") {
        Some(name) => {
            let n = name.trim();
            n.is_empty() || n == "?" || n.eq_ignore_ascii_case("unknown")
        }
        None => node_id.trim().is_empty(),
    }
}

fn generate_cmd_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("cmd-{}", millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_contains_c2_and_cluster_entities() {
        let campaign = Campaign::bootstrap(
            "Ran",
            K8sCluster::new("dev-cluster")
                .with_context_name(Some("dev-context".to_string()))
                .with_server(Some("https://127.0.0.1:6443".to_string())),
        );

        assert_eq!(campaign.entity_count(), 2);
        assert!(campaign.c2_servers.contains_key(&EntityId::new("c2/ran")));
        assert!(campaign
            .clusters
            .contains_key(&EntityId::new("k8s/cluster/dev-cluster")));
    }
}
