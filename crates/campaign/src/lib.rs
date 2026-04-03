use std::collections::HashMap;

use armory::{Armory, Procedure, Ttp};
use c2::{ExecTtp, TtpExecuted};
use ran_domain::{
    C2Server, Entity, EntityId, K8sCluster, K8sNode, Namespace, Pod, RelationSummary,
    ServiceAccount, SystemEntity,
};
use serde::{Deserialize, Serialize};

use external_parser::SystemFieldUpdates;

#[cfg(test)]
mod analyzers;
pub mod effects;
pub mod external_parser;
pub mod failure_analyzers;
pub mod output_parsers;
pub mod rules;
pub mod runtime;
pub use effects::FactsUpdate;
pub use external_parser::{ExternalParseRequest, ExternalParseResponse, ExternalParser};
pub use output_parsers::{ParseAudit, ParseResult};
pub use rules::{default_rules, run_rules_fixpoint, InferenceRule, RuleTrigger};
pub use runtime::{spawn_c2_event_processor, spawn_c2_event_processor_with_external_parser, CampaignEvent, CampaignEventBus, EntitySummary};
use effects::{ground_template, parse_effect_with_status};
use failure_analyzers::{classify_failure, FAILURE_ANALYZER_EFFECT_ID};
use output_parsers::{build_no_parser_audit, build_parse_audit, parse_output_effect};
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

pub enum CampaignSystemEntityRef<'a> {
    Node(&'a K8sNode),
    Pod(&'a Pod),
}

impl<'a> CampaignSystemEntityRef<'a> {
    pub fn entity(&self) -> &'a dyn SystemEntity {
        match self {
            CampaignSystemEntityRef::Node(e) => *e,
            CampaignSystemEntityRef::Pod(e) => *e,
        }
    }
}

pub enum CampaignSystemEntityMut<'a> {
    Node(&'a mut K8sNode),
    Pod(&'a mut Pod),
}

impl<'a> CampaignSystemEntityMut<'a> {
    pub fn entity_mut(&mut self) -> &mut dyn SystemEntity {
        match self {
            CampaignSystemEntityMut::Node(e) => *e,
            CampaignSystemEntityMut::Pod(e) => *e,
        }
    }
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
    pub parse_audits: Vec<ParseAudit>,
}

#[derive(Default)]
pub struct TtpExecutionProcessing {
    pub updates: FactsUpdate,
    pub parse_audits: Vec<ParseAudit>,
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
            parse_audits: Vec::new(),
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

    pub fn get_parse_audits(&self) -> &[ParseAudit] {
        &self.parse_audits
    }

    pub fn get_system_entity(&self, id: &str) -> Option<CampaignSystemEntityRef<'_>> {
        let entity_id = EntityId::new(id);

        if let Some(node) = self.nodes.get(&entity_id) {
            return Some(CampaignSystemEntityRef::Node(node));
        }

        self.pods
            .get(&entity_id)
            .map(CampaignSystemEntityRef::Pod)
    }

    pub fn get_system_entity_mut(&mut self, id: &str) -> Option<CampaignSystemEntityMut<'_>> {
        let entity_id = EntityId::new(id);

        if let Some(node) = self.nodes.get_mut(&entity_id) {
            return Some(CampaignSystemEntityMut::Node(node));
        }

        self.pods
            .get_mut(&entity_id)
            .map(CampaignSystemEntityMut::Pod)
    }

    /// Apply partial system-info updates from an external parser to a target
    /// entity.  Returns the number of new facts written, or an error if the
    /// target is not a system entity.
    pub fn apply_system_update(
        &mut self,
        target_id: &str,
        updates: &SystemFieldUpdates,
    ) -> Result<usize, String> {
        let Some(mut target) = self.get_system_entity_mut(target_id) else {
            return Err(format!(
                "target '{}' is not a system entity",
                target_id
            ));
        };
        let sys = target.entity_mut().system_mut();
        Ok(external_parser::apply_system_field_updates(sys, updates))
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
    ) -> Result<TtpExecutionProcessing, ExecuteActionError> {
        let mut updates = FactsUpdate::default();
        let mut parse_audits = Vec::new();

        if !event.success {
            let classified = classify_failure(cmd, event);
            parse_audits.push(build_parse_audit(
                FAILURE_ANALYZER_EFFECT_ID,
                cmd,
                event,
                classified.parse_result,
                &classified.detail,
                0,
            ));

            self.parse_audits.extend(parse_audits.clone());
            return Ok(TtpExecutionProcessing {
                updates,
                parse_audits,
            });
        }

        for effect in &cmd.ttp.effects {
            if let Some(parsed_output) = parse_output_effect(self, effect, cmd, event) {
                updates.merge(parsed_output.updates);
                parse_audits.push(parsed_output.audit);
                continue;
            }

            match parse_effect_with_status(effect, &cmd.args) {
                Ok(parsed_structural) if parsed_structural.handled => {
                    updates.merge(parsed_structural.updates);
                    parse_audits.push(build_parse_audit(
                        effect,
                        cmd,
                        event,
                        ParseResult::Parsed,
                        "parsed by structural effect handler",
                        0,
                    ));
                }
                Ok(_) => {
                    parse_audits.push(build_no_parser_audit(effect, cmd, event));
                }
                Err(err) => {
                    parse_audits.push(build_parse_audit(
                        effect,
                        cmd,
                        event,
                        ParseResult::ParserBug,
                        &err,
                        0,
                    ));
                }
            }
        }

        let rules = make_rules();
        updates = run_rules_fixpoint(self, &rules, updates);

        self.apply_facts(&updates);
        self.parse_audits.extend(parse_audits.clone());

        Ok(TtpExecutionProcessing {
            updates,
            parse_audits,
        })
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
    use std::collections::HashMap;

    use armory::{Procedure, Ttp};
    use c2::{ExecTtp, TtpExecuted};

    use super::*;

    fn sample_exec_ttp(target_id: &str, effects: Vec<&str>) -> ExecTtp {
        ExecTtp {
            id: "cmd-1".to_string(),
            ttp: Ttp {
                id: "ttp-test".to_string(),
                name: "Test TTP".to_string(),
                description: "test".to_string(),
                tactic: "Discovery".to_string(),
                techniques: vec![],
                status: "stable".to_string(),
                params: vec![],
                requires: Default::default(),
                effects: effects.into_iter().map(str::to_string).collect(),
                procedures: vec![Procedure {
                    id: "shell".to_string(),
                    command: "env".to_string(),
                    tool: None,
                    is_local_command: None,
                }],
                references: vec![],
            },
            procedure: Procedure {
                id: "shell".to_string(),
                command: "env".to_string(),
                tool: None,
                is_local_command: None,
            },
            args: HashMap::new(),
            target_id: target_id.to_string(),
            exec_system_id: String::new(),
        }
    }

    fn sample_event(stdout: &str) -> TtpExecuted {
        TtpExecuted {
            id: "evt-1".to_string(),
            success: true,
            results: vec![stdout.to_string(), String::new()],
            exit_code: 0,
            fail_reason: String::new(),
        }
    }

    fn sample_failed_event(fail_reason: &str) -> TtpExecuted {
        TtpExecuted {
            id: "evt-1".to_string(),
            success: false,
            results: vec![fail_reason.to_string()],
            exit_code: 1,
            fail_reason: fail_reason.to_string(),
        }
    }

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

    #[test]
    fn on_ttp_executed_records_no_parser_audit_for_unknown_effect() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
        let pod = Pod::new("demo", "default");
        let target_id = pod.entity_id().0;
        campaign.pods.insert(pod.entity_id(), pod);

        let cmd = sample_exec_ttp(&target_id, vec!["sys.unknown"]);
        let event = sample_event("X=1\n");

        let processed = campaign.on_ttp_executed(&cmd, &event).unwrap();

        assert_eq!(processed.parse_audits.len(), 1);
        assert!(matches!(
            processed.parse_audits[0].parse_result,
            ParseResult::NoParser
        ));
    }

    #[test]
    fn on_ttp_executed_parses_sys_envvar_into_target_system_info() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
        let pod = Pod::new("demo", "default");
        let target_id = pod.entity_id().0;
        campaign.pods.insert(pod.entity_id(), pod);

        let cmd = sample_exec_ttp(&target_id, vec!["sys.envvar"]);
        let event = sample_event("HOME=/root\nPATH=/usr/bin\n");

        let processed = campaign.on_ttp_executed(&cmd, &event).unwrap();

        assert_eq!(processed.parse_audits.len(), 1);
        assert!(matches!(
            processed.parse_audits[0].parse_result,
            ParseResult::Parsed
        ));

        let target = campaign.get_system_entity(&target_id).unwrap();
        let env_vars = &target.entity().system().env_vars;
        assert_eq!(env_vars.get("HOME"), Some(&"/root".to_string()));
        assert_eq!(env_vars.get("PATH"), Some(&"/usr/bin".to_string()));
    }

    #[test]
    fn on_ttp_executed_failure_records_known_failure_audit() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
        let cmd = sample_exec_ttp("ns/default/pod", vec!["sys.envvar"]);
        let event = sample_failed_event(
            "invalid pod target id 'ns/default/pod': expected format ns/<namespace>/pod/<pod-name>",
        );

        let processed = campaign.on_ttp_executed(&cmd, &event).unwrap();

        assert_eq!(processed.parse_audits.len(), 1);
        assert!(matches!(
            processed.parse_audits[0].parse_result,
            ParseResult::KnownFailure
        ));
        assert_eq!(
            processed.parse_audits[0].effect_id,
            FAILURE_ANALYZER_EFFECT_ID
        );
    }

    #[test]
    fn on_ttp_executed_failure_records_unknown_format_when_unclassified() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev-cluster"));
        let cmd = sample_exec_ttp("ns/default/pod/demo", vec!["sys.envvar"]);
        let event = sample_failed_event("something odd happened");

        let processed = campaign.on_ttp_executed(&cmd, &event).unwrap();

        assert_eq!(processed.parse_audits.len(), 1);
        assert!(matches!(
            processed.parse_audits[0].parse_result,
            ParseResult::UnknownFormat
        ));
        assert_eq!(
            processed.parse_audits[0].effect_id,
            FAILURE_ANALYZER_EFFECT_ID
        );
    }
}
