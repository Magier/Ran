use std::collections::{BTreeSet, HashMap};

use indexmap::IndexSet;
use ran_domain::{
    CanReach, ContainerEscape, CronJob, Entity, EntityId, K8sNode, K8sRole, K8sRoleBinding,
    KubeletExecSource, NameConfidence, OutputTransformKind, Pod, PodExec, RbacPermission,
    RbacSubject, RceCanExec, Relation, RunsOn, ServiceAccount, SessionChannel,
};

use crate::grounding::resolve_template;
use crate::{KnowledgeProvenance, RelationProvenanceKey};

type SimpleEffectHandler = fn(&HashMap<String, String>) -> Result<FactsUpdate, String>;
/// Handler for relation-style effects such as `rce.can-exec(src, tgt)`.
/// Receives both the positional args (from parsing the effect string) and the
/// full execution context (the TTP args map) so handlers that need extra
/// context — like `PROCEDURE_CMD` for `rce.can-exec` — can read it without
/// requiring a separate post-hoc injection step.
type RelationEffectHandler = fn(&[&str], &HashMap<String, String>) -> Result<FactsUpdate, String>;

pub struct ParsedStructuralEffect {
    pub updates: FactsUpdate,
    pub handled: bool,
}

#[derive(Debug, Default)]
pub struct FactsUpdate {
    pub new_entities: Vec<Box<dyn Entity + Send + Sync>>,
    pub new_relations: Vec<Box<dyn Relation + Send + Sync>>,
    /// Entity-identity merges: `(stale_id, preferred_id)`.
    ///
    /// When applied, all relations referencing `stale_id` are rewritten to
    /// `preferred_id`, the stale entity's runtime data is merged into the
    /// preferred entity, and the stale entity is removed from campaign state.
    /// Used when a placeholder entity (e.g. IP-derived pod name from a network
    /// scan) is later identified as an already-known named entity.
    pub entity_aliases: IndexSet<(EntityId, EntityId)>,
    pub entity_provenance: HashMap<EntityId, BTreeSet<KnowledgeProvenance>>,
    pub relation_provenance: HashMap<RelationProvenanceKey, BTreeSet<KnowledgeProvenance>>,
}

impl FactsUpdate {
    pub fn merge(&mut self, other: Self) {
        let FactsUpdate {
            new_entities,
            new_relations,
            entity_aliases,
            entity_provenance,
            relation_provenance,
        } = other;
        // Build O(1)-lookup sets from existing entries so each item from `other`
        // is checked in O(1) rather than O(n), avoiding the previous O(n²) scan.
        let seen_entities: IndexSet<EntityId> =
            self.new_entities.iter().map(|e| e.entity_id()).collect();
        for entity in new_entities {
            if !seen_entities.contains(&entity.entity_id()) {
                self.new_entities.push(entity);
            }
        }

        let seen_relations: IndexSet<(String, EntityId, EntityId)> = self
            .new_relations
            .iter()
            .map(|r| {
                (
                    r.relation_name().to_string(),
                    r.source_id().clone(),
                    r.target_id().clone(),
                )
            })
            .collect();
        for rel in new_relations {
            let key = (
                rel.relation_name().to_string(),
                rel.source_id().clone(),
                rel.target_id().clone(),
            );
            if !seen_relations.contains(&key) {
                self.new_relations.push(rel);
            }
        }

        // IndexSet::insert handles dedup natively — no scan needed.
        self.entity_aliases.extend(entity_aliases);
        for (id, origins) in entity_provenance {
            self.entity_provenance
                .entry(id)
                .or_default()
                .extend(origins);
        }
        for (key, origins) in relation_provenance {
            self.relation_provenance
                .entry(key)
                .or_default()
                .extend(origins);
        }
    }

    pub fn attribute_unattributed(&mut self, provenance: KnowledgeProvenance) {
        for entity in &self.new_entities {
            self.entity_provenance
                .entry(entity.entity_id())
                .or_default()
                .insert(provenance);
        }
        for relation in &self.new_relations {
            self.relation_provenance
                .entry(RelationProvenanceKey::from_relation(relation.as_ref()))
                .or_default()
                .insert(provenance);
        }
    }
}

pub fn ground_template(template: &str, args: &HashMap<String, String>) -> String {
    // Pass 1: evaluate template {% if/else/endif %} blocks and {{ Var }} substitutions.
    let mut grounded = resolve_template(template, args);

    // Pass 2: replace ${KEY} placeholders with case-insensitive matching on
    // the placeholder name itself (e.g. ${SRC}, ${src}, ${Src}).
    grounded = substitute_dollar_placeholders_case_insensitive(&grounded, args);

    grounded
}

fn substitute_dollar_placeholders_case_insensitive(
    template: &str,
    args: &HashMap<String, String>,
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut idx = 0usize;

    while let Some(rel_start) = template[idx..].find("${") {
        let start = idx + rel_start;
        out.push_str(&template[idx..start]);

        let name_start = start + 2;
        let Some(rel_end) = template[name_start..].find('}') else {
            // Unterminated placeholder; keep remainder verbatim.
            out.push_str(&template[start..]);
            return out;
        };
        let end = name_start + rel_end;
        let placeholder_name = &template[name_start..end];

        if let Some((_, value)) = args
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(placeholder_name))
        {
            out.push_str(value);
        } else {
            out.push_str(&template[start..=end]);
        }

        idx = end + 1;
    }

    out.push_str(&template[idx..]);
    out
}

pub fn parse_effect(effect: &str, args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    Ok(parse_effect_with_status(effect, args)?.updates)
}

pub fn parse_effect_with_status(
    effect: &str,
    args: &HashMap<String, String>,
) -> Result<ParsedStructuralEffect, String> {
    let normalized = effect.trim();

    if let Some(handler) = resolve_simple_effect_handler(normalized) {
        return Ok(ParsedStructuralEffect {
            updates: handler(args)?,
            handled: true,
        });
    }

    if normalized.contains('(') && normalized.ends_with(')') {
        return parse_relation_effect(normalized, args);
    }

    Ok(ParsedStructuralEffect {
        updates: FactsUpdate::default(),
        handled: false,
    })
}

fn parse_k8s_pod(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"])
        .ok_or_else(|| "k8s.Pod effect requires Namespace argument".to_string())?;

    let pod_name = get_arg(args, &["PodName", "PODNAME", "POD_NAME"])
        .ok_or_else(|| "k8s.Pod effect requires PodName argument".to_string())?;

    let mut pod = Pod::new(pod_name, namespace);

    if let Some(node_name) = get_arg(args, &["NodeName", "NODENAME", "NODE_NAME", "Node", "NODE"]) {
        if !node_name.trim().is_empty() {
            pod.node_name = Some(node_name.to_string());
        }
    }

    if let Some(sa_name) = get_arg(
        args,
        &[
            "ServiceAccount",
            "SERVICEACCOUNT",
            "SERVICE_ACCOUNT",
            "ServiceAccountName",
        ],
    ) {
        if !sa_name.trim().is_empty() {
            pod.service_account_name = Some(sa_name.to_string());
        }
    }

    if let Some(is_running) = get_arg(args, &["IsRunning", "ISRUNNING", "IS_RUNNING"]) {
        pod.is_running = parse_bool_like(is_running);
    }

    Ok(FactsUpdate {
        new_entities: vec![Box::new(pod)],
        new_relations: Vec::new(),
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn parse_k8s_serviceaccount(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"])
        .ok_or_else(|| "k8s.serviceaccount effect requires Namespace argument".to_string())?;

    let sa_name = get_arg(
        args,
        &["ServiceAccountName", "SA_NAME", "SERVICEACCOUNTNAME"],
    )
    .ok_or_else(|| "k8s.serviceaccount effect requires ServiceAccountName argument".to_string())?;

    let mut sa = ServiceAccount::new(sa_name, namespace);

    if let Some(raw_token) = get_arg(args, &["Token", "TOKEN"]) {
        if !raw_token.is_empty() {
            use ran_domain::{JwToken, ServiceAccountToken};
            sa.token = Some(ServiceAccountToken {
                jwt: JwToken {
                    raw: raw_token.to_string(),
                    ..Default::default()
                },
                namespace: namespace.to_string(),
                service_account_name: sa_name.to_string(),
                ..Default::default()
            });
        }
    }

    Ok(FactsUpdate {
        new_entities: vec![Box::new(sa)],
        new_relations: Vec::new(),
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn parse_k8s_role(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"])
        .ok_or_else(|| "k8s.role effect requires Namespace argument".to_string())?;

    let role_name = get_arg(args, &["RoleName", "ROLENAME", "ROLE_NAME"])
        .ok_or_else(|| "k8s.role effect requires RoleName argument".to_string())?;

    let mut role = K8sRole::new(role_name, namespace);

    if let Some(rules_json) = get_arg(args, &["Rules", "RULES"]) {
        role.permissions = parse_rules_json(rules_json);
    }

    Ok(FactsUpdate {
        new_entities: vec![Box::new(role)],
        new_relations: Vec::new(),
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn parse_k8s_rolebinding(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"])
        .ok_or_else(|| "k8s.rolebinding effect requires Namespace argument".to_string())?;

    let binding_name = get_arg(args, &["BindingName", "BINDINGNAME", "BINDING_NAME"])
        .ok_or_else(|| "k8s.rolebinding effect requires BindingName argument".to_string())?;

    let mut binding = K8sRoleBinding::new(binding_name, namespace);

    if let Some(role_ref) = get_arg(args, &["RoleRef", "ROLEREF", "ROLE_REF"]) {
        binding.role_ref = role_ref.to_string();
    }

    if let Some(subjects_json) = get_arg(args, &["Subjects", "SUBJECTS"]) {
        binding.subjects = parse_subjects_json(subjects_json);
    }

    Ok(FactsUpdate {
        new_entities: vec![Box::new(binding)],
        new_relations: Vec::new(),
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn parse_k8s_cronjob(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"])
        .ok_or_else(|| "k8s.cronjob effect requires Namespace argument".to_string())?;

    let name = get_arg(args, &["CronJobName", "CRONJOBNAME", "CRONJOB_NAME"])
        .ok_or_else(|| "k8s.cronjob effect requires CronJobName argument".to_string())?;

    let mut cj = CronJob::new(name, namespace);

    if let Some(schedule) = get_arg(args, &["Schedule", "SCHEDULE"]) {
        if !schedule.is_empty() {
            cj.schedule = Some(schedule.to_string());
        }
    }

    Ok(FactsUpdate {
        new_entities: vec![Box::new(cj)],
        new_relations: Vec::new(),
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

/// Parse a JSON array of RBAC rule objects into `RbacPermission` entries.
///
/// Expected format (same schema as `k8s.selfsubjectrulesreview` output):
/// ```json
/// [{"verbs":["get","list"],"resources":["pods"],"apiGroups":[""]}]
/// ```
fn parse_rules_json(json: &str) -> Vec<RbacPermission> {
    let json = json.trim();
    if json.is_empty() || json == "[]" || json == "null" {
        return Vec::new();
    }

    #[derive(serde::Deserialize)]
    struct RuleEntry {
        #[serde(default)]
        verbs: Vec<String>,
        #[serde(default)]
        resources: Vec<String>,
        #[serde(rename = "apiGroups", default)]
        api_groups: Vec<String>,
    }

    let Ok(entries) = serde_json::from_str::<Vec<RuleEntry>>(json) else {
        return Vec::new();
    };

    let mut perms = Vec::new();
    for entry in entries {
        for verb in &entry.verbs {
            for resource in &entry.resources {
                let mut perm = RbacPermission::new(verb.clone(), resource.clone());
                perm.api_group = entry.api_groups.first().cloned();
                perms.push(perm);
            }
        }
    }
    perms
}

/// Parse a JSON array of subject objects into `RbacSubject` entries.
///
/// Expected format:
/// ```json
/// [{"kind":"ServiceAccount","name":"my-sa","namespace":"default"}]
/// ```
fn parse_subjects_json(json: &str) -> Vec<RbacSubject> {
    let json = json.trim();
    if json.is_empty() || json == "[]" || json == "null" {
        return Vec::new();
    }

    serde_json::from_str::<Vec<RbacSubject>>(json).unwrap_or_default()
}

fn parse_relation_effect(
    effect: &str,
    ctx: &HashMap<String, String>,
) -> Result<ParsedStructuralEffect, String> {
    let (name, args) = split_relation(effect)?;

    if let Some(handler) = resolve_relation_effect_handler(name) {
        return Ok(ParsedStructuralEffect {
            updates: handler(&args, ctx)?,
            handled: true,
        });
    }

    Ok(ParsedStructuralEffect {
        updates: FactsUpdate::default(),
        handled: false,
    })
}

/// The canonical taxonomy of effects the campaign understands.
///
/// This is the **single source of truth** for effect names: both the parser
/// (which maps a kind to a [`FactsUpdate`]-producing handler below) and the
/// action scorer (which maps a kind to a value via [`EffectKind::categories`])
/// resolve through [`EffectKind::parse`]. Adding a new effect kind is one edit
/// here that the exhaustive `categories` match forces you to classify — a kind
/// can never be parseable yet unvalued, so the two never drift.
///
/// The set is intentionally **not closed**: more effects will be added. An
/// effect string that doesn't match any variant returns `None` (fail-soft —
/// the parser treats it as `handled: false`, the scorer values it at zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    // Simple, entity-producing effects (have a structural FactsUpdate handler).
    K8sPod,
    K8sServiceAccount,
    K8sRole,
    K8sRoleBinding,
    K8sCronJob,
    // Resource enumeration / discovery (parser-driven; no structural handler,
    // but still part of the vocabulary so the scorer can value them).
    PodList,
    NodeList,
    ServiceList,
    ServiceAccountList,
    SecretList,
    RoleList,
    RoleBindingList,
    ClusterRoleList,
    ClusterRoleBindingList,
    ConfigMapList,
    DeploymentList,
    IngressList,
    HttpRouteList,
    GatewayList,
    SelfSubjectRulesReview,
    RawServiceAccountToken,
    // Identity facts — learning a named entity's identity (e.g. discovered
    // alongside a token read). Distinct from the `k8s.*` entity-creating effects.
    PodName,
    ServiceAccountName,
    NamespaceName,
    // Host / system discovery.
    SysFiles,
    SysProcesses,
    SysIp,
    SysHasFile,
    SysHasBinary,
    LinuxMounts,
    ReverseDns,
    FileContent,
    FileKubeconfig,
    // Relation-producing effects.
    K8sCanExec,
    K8sCanReach,
    RunsOn,
    KubeletExecSource,
    C2Session,
    RceCanExec,
    ContainerEscape,
    // Imperative RBAC creation — privilege escalation.
    CreateRole,
    CreateRoleBinding,
}

/// What executing an effect *does to the belief state* — the basis the scorer
/// derives value from, so effects sharing a category are valued consistently.
/// An effect may fall in more than one category (e.g. enumerating Services is
/// both `Discovery` and `Reachability`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectCategory {
    /// Adds entities/facts about the world — reduces uncertainty (POMDP).
    Discovery,
    /// Adds execution or escape capability — raises effective privilege.
    PrivilegeEdge,
    /// Adds a session/route or reveals network & adjacent entities — extends
    /// where we can operate next.
    Reachability,
}

impl EffectKind {
    /// Parse an effect string (with or without a `(args)` suffix) into its kind.
    /// Returns `None` for effects outside the current taxonomy.
    pub fn parse(effect: &str) -> Option<Self> {
        // Drop any relation argument list, then normalize the bare name.
        let name = effect.trim();
        let name = name.split('(').next().unwrap_or(name).trim();
        let kind = match name.to_ascii_lowercase().as_str() {
            "k8s.pod" => Self::K8sPod,
            "k8s.serviceaccount" => Self::K8sServiceAccount,
            "k8s.role" => Self::K8sRole,
            "k8s.rolebinding" => Self::K8sRoleBinding,
            "k8s.cronjob" => Self::K8sCronJob,
            "k8s.podlist" => Self::PodList,
            "k8s.nodelist" => Self::NodeList,
            "k8s.servicelist" => Self::ServiceList,
            "k8s.serviceaccountlist" => Self::ServiceAccountList,
            "k8s.secretlist" => Self::SecretList,
            "k8s.rolelist" => Self::RoleList,
            "k8s.rolebindinglist" => Self::RoleBindingList,
            "k8s.clusterrolelist" => Self::ClusterRoleList,
            "k8s.clusterrolebindinglist" => Self::ClusterRoleBindingList,
            "k8s.configmaplist" => Self::ConfigMapList,
            "k8s.deploymentlist" => Self::DeploymentList,
            "k8s.ingresslist" => Self::IngressList,
            "k8s.httproutelist" => Self::HttpRouteList,
            "k8s.gatewaylist" => Self::GatewayList,
            "k8s.selfsubjectrulesreview" => Self::SelfSubjectRulesReview,
            "rawserviceaccounttoken" => Self::RawServiceAccountToken,
            "pod.name" => Self::PodName,
            "serviceaccount.name" => Self::ServiceAccountName,
            "namespace.name" => Self::NamespaceName,
            "sys.files" => Self::SysFiles,
            "sys.processes" => Self::SysProcesses,
            "sys.ip" => Self::SysIp,
            "sys.hasfile" => Self::SysHasFile,
            "sys.hasbinary" | "sys.has-binary" => Self::SysHasBinary,
            "linux.mounts" => Self::LinuxMounts,
            "rdns" => Self::ReverseDns,
            "file:content" => Self::FileContent,
            "file:kubeconfig" => Self::FileKubeconfig,
            "k8s.can-exec" => Self::K8sCanExec,
            "k8s.can-reach" => Self::K8sCanReach,
            "k8s.runs-on" | "runs-on" => Self::RunsOn,
            "k8s.kubelet-exec-source" | "k8s.kubelet-exec" => Self::KubeletExecSource,
            "c2.session" => Self::C2Session,
            "rce.can-exec" => Self::RceCanExec,
            "container.escape" => Self::ContainerEscape,
            "create k8s.role" => Self::CreateRole,
            "create k8s.rolebinding" => Self::CreateRoleBinding,
            _ => return None,
        };
        Some(kind)
    }

    /// The categories this effect contributes to. Exhaustive by construction —
    /// adding a variant forces a classification here, so value can't drift.
    pub fn categories(self) -> &'static [EffectCategory] {
        use EffectCategory::{Discovery, PrivilegeEdge, Reachability};
        match self {
            // Execution / escape capability, plus RBAC self-grants.
            Self::K8sCanExec
            | Self::KubeletExecSource
            | Self::RceCanExec
            | Self::ContainerEscape
            | Self::CreateRole
            | Self::CreateRoleBinding => &[PrivilegeEdge],
            // Pure network reach.
            Self::C2Session | Self::K8sCanReach => &[Reachability],
            // Network & adjacent-entity discovery — informs both *what's out
            // there* and *where we can move next*.
            Self::K8sPod
            | Self::PodList
            | Self::NodeList
            | Self::ServiceList
            | Self::IngressList
            | Self::HttpRouteList
            | Self::GatewayList
            | Self::DeploymentList
            | Self::ReverseDns
            | Self::SysIp
            | Self::RunsOn => &[Discovery, Reachability],
            // Pure discovery (RBAC, secrets, config, host facts, credentials).
            Self::K8sServiceAccount
            | Self::K8sRole
            | Self::K8sRoleBinding
            | Self::K8sCronJob
            | Self::ServiceAccountList
            | Self::SecretList
            | Self::RoleList
            | Self::RoleBindingList
            | Self::ClusterRoleList
            | Self::ClusterRoleBindingList
            | Self::ConfigMapList
            | Self::SelfSubjectRulesReview
            | Self::RawServiceAccountToken
            | Self::PodName
            | Self::ServiceAccountName
            | Self::NamespaceName
            | Self::SysFiles
            | Self::SysProcesses
            | Self::SysHasFile
            | Self::SysHasBinary
            | Self::LinuxMounts
            | Self::FileContent
            | Self::FileKubeconfig => &[Discovery],
        }
    }

    /// How broadly the knowledge this effect produces tends to enable *other*
    /// actions — a static, per-effect prior (not a count of consumers, so it
    /// stays decoupled from the rest of the armory). Used to weight discovery
    /// value: foundational facts (an IP, a token, an identity) ground many
    /// downstream actions; specialized ones (a capability check, one file's
    /// contents) ground almost none.
    ///
    /// The anchor for "foundational" is non-arbitrary: these are the facts the
    /// grounding system injects as well-known variables (IP, token, namespace,
    /// pod/node identity). Only meaningful for `Discovery` effects; the scorer
    /// queries it after filtering to that category.
    pub fn generality(self) -> f32 {
        match self {
            // Foundational: identity / address / credential facts that feed
            // grounding variables and thus enable many later actions.
            Self::SysIp
            | Self::ReverseDns
            | Self::K8sPod
            | Self::NodeList
            | Self::RunsOn
            | Self::K8sServiceAccount
            | Self::RawServiceAccountToken
            | Self::PodName
            | Self::ServiceAccountName
            | Self::NamespaceName
            | Self::FileKubeconfig => GENERALITY_FOUNDATIONAL,
            // Standard: resource enumerations and common host facts.
            Self::PodList
            | Self::ServiceList
            | Self::ServiceAccountList
            | Self::SecretList
            | Self::RoleList
            | Self::RoleBindingList
            | Self::ClusterRoleList
            | Self::ClusterRoleBindingList
            | Self::ConfigMapList
            | Self::DeploymentList
            | Self::IngressList
            | Self::HttpRouteList
            | Self::GatewayList
            | Self::K8sRole
            | Self::K8sRoleBinding
            | Self::K8sCronJob
            | Self::SysFiles
            | Self::SysProcesses => GENERALITY_STANDARD,
            // Specialized: narrow, single-purpose facts.
            Self::SysHasFile
            | Self::SysHasBinary
            | Self::LinuxMounts
            | Self::SelfSubjectRulesReview
            | Self::FileContent => GENERALITY_SPECIALIZED,
            // Non-discovery effects: generality is unused (the scorer filters to
            // Discovery before querying it); neutral value for completeness.
            Self::K8sCanExec
            | Self::K8sCanReach
            | Self::KubeletExecSource
            | Self::C2Session
            | Self::RceCanExec
            | Self::ContainerEscape
            | Self::CreateRole
            | Self::CreateRoleBinding => GENERALITY_STANDARD,
        }
    }

    /// Whether the fact this effect produces can go **stale** — i.e. its answer
    /// changes over time or as other actions mutate the cluster.
    ///
    /// `false` (stable / idempotent): point-in-time facts that don't change
    /// without deliberate action — an IP, a hostname, an identity, a capability,
    /// an achieved capability (exec/escape). Re-learning them yields nothing.
    ///
    /// `true` (volatile): set memberships and mutable state — resource
    /// enumerations, the current RBAC view, running processes, directory
    /// listings. These can be invalidated by later actions, so re-reading can
    /// regain epistemic value. Tunable per-effect, like the generality tiers.
    pub fn is_volatile(self) -> bool {
        match self {
            // Volatile: enumerations and mutable state.
            Self::PodList
            | Self::NodeList
            | Self::ServiceList
            | Self::ServiceAccountList
            | Self::SecretList
            | Self::RoleList
            | Self::RoleBindingList
            | Self::ClusterRoleList
            | Self::ClusterRoleBindingList
            | Self::ConfigMapList
            | Self::DeploymentList
            | Self::IngressList
            | Self::HttpRouteList
            | Self::GatewayList
            | Self::SelfSubjectRulesReview
            | Self::SysProcesses
            | Self::SysFiles => true,
            // Stable / idempotent: identities, addresses, capabilities, and
            // achieved-capability relations.
            Self::K8sPod
            | Self::K8sServiceAccount
            | Self::K8sRole
            | Self::K8sRoleBinding
            | Self::K8sCronJob
            | Self::RawServiceAccountToken
            | Self::PodName
            | Self::ServiceAccountName
            | Self::NamespaceName
            | Self::SysIp
            | Self::SysHasFile
            | Self::SysHasBinary
            | Self::LinuxMounts
            | Self::ReverseDns
            | Self::FileContent
            | Self::FileKubeconfig
            | Self::K8sCanExec
            | Self::K8sCanReach
            | Self::RunsOn
            | Self::KubeletExecSource
            | Self::C2Session
            | Self::RceCanExec
            | Self::ContainerEscape
            | Self::CreateRole
            | Self::CreateRoleBinding => false,
        }
    }
}

/// Generality tiers — how broadly a produced fact enables further actions.
/// Tunable: raise/lower to change how much foundational discoveries outrank
/// specialized ones.
const GENERALITY_FOUNDATIONAL: f32 = 1.0;
const GENERALITY_STANDARD: f32 = 0.6;
const GENERALITY_SPECIALIZED: f32 = 0.3;

fn resolve_simple_effect_handler(effect_name: &str) -> Option<SimpleEffectHandler> {
    match EffectKind::parse(effect_name)? {
        EffectKind::K8sPod => Some(parse_k8s_pod),
        EffectKind::K8sServiceAccount => Some(parse_k8s_serviceaccount),
        EffectKind::K8sRole => Some(parse_k8s_role),
        EffectKind::K8sRoleBinding => Some(parse_k8s_rolebinding),
        EffectKind::K8sCronJob => Some(parse_k8s_cronjob),
        // Relation-producing kinds have no simple handler.
        _ => None,
    }
}

fn resolve_relation_effect_handler(effect_name: &str) -> Option<RelationEffectHandler> {
    match EffectKind::parse(effect_name)? {
        EffectKind::K8sCanExec => Some(parse_k8s_can_exec_relation),
        EffectKind::K8sCanReach => Some(parse_k8s_can_reach_relation),
        EffectKind::RunsOn => Some(parse_runs_on_relation),
        EffectKind::KubeletExecSource => Some(parse_kubelet_exec_source_relation),
        EffectKind::C2Session => Some(parse_c2_session_relation),
        EffectKind::RceCanExec => Some(parse_rce_can_exec_relation),
        EffectKind::ContainerEscape => Some(parse_container_escape_relation),
        // Entity-producing kinds have no relation handler.
        _ => None,
    }
}

fn parse_c2_session_relation(
    args: &[&str],
    ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("c2.session effect expects exactly 2 args: backend and target".to_string());
    }

    let backend_raw = args[0].trim();
    if backend_raw.is_empty() {
        return Err("c2.session backend cannot be empty".to_string());
    }

    // `sys` resolves to the entity the TTP executed on.
    let target_id = if args[1].eq_ignore_ascii_case("sys") {
        ctx.get("TARGET_ID")
            .filter(|id| !id.is_empty())
            .map(String::as_str)
            .ok_or_else(|| "c2.session: 'sys' requires TARGET_ID in context".to_string())?
    } else {
        args[1]
    };

    // First arg accepts one of:
    // - `sliver`            => source `c2/sliver`, session backend `session/sliver`
    // - `c2/sliver`         => source `c2/sliver`, session backend `session/c2/sliver`
    // - `session/sliver-1`  => source `c2/sliver-1`, session backend `session/sliver-1`
    let (source_id, session_id) = if let Some(rest) = backend_raw.strip_prefix("session/") {
        let source = if rest.starts_with("c2/") {
            rest.to_string()
        } else {
            format!("c2/{}", rest)
        };
        (source, backend_raw.to_string())
    } else if backend_raw.starts_with("c2/") {
        (backend_raw.to_string(), format!("session/{}", backend_raw))
    } else {
        (
            format!("c2/{}", backend_raw),
            format!("session/{}", backend_raw),
        )
    };

    let rel = SessionChannel::new(source_id, target_id, session_id);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn parse_k8s_can_exec_relation(
    args: &[&str],
    _ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("k8s.can-exec effect expects exactly 2 args".to_string());
    }
    let rel = PodExec::new(args[0], args[1]);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn parse_k8s_can_reach_relation(
    args: &[&str],
    _ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("k8s.can-reach effect expects exactly 2 args".to_string());
    }
    let rel = CanReach::new(args[0], args[1]);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn parse_runs_on_relation(
    args: &[&str],
    _ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("runs-on effect expects exactly 2 args".to_string());
    }
    let rel = RunsOn::new(args[0], args[1]);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn parse_kubelet_exec_source_relation(
    args: &[&str],
    ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("kubelet-exec effect expects exactly 2 args".to_string());
    }
    // `sys` is a well-known placeholder for the entity that executed the TTP.
    // Resolve it to the actual target entity ID stored in the context.
    let src = if args[0].eq_ignore_ascii_case("sys") {
        ctx.get("TARGET_ID")
            .filter(|id| !id.is_empty())
            .map(String::as_str)
            .ok_or_else(|| {
                "kubelet-exec: arg is 'sys' but TARGET_ID not present in context".to_string()
            })?
    } else {
        args[0]
    };
    // Use PROCEDURE_CMD as the envelope template so subsequent command routing
    // can wrap via RelationSummary::wrap_command with ${CMD} substitution.
    let envelope = ctx
        .get("PROCEDURE_CMD")
        .filter(|v| !v.trim().is_empty())
        .cloned();

    let tgt_raw = args[1].trim();
    let mut rel = KubeletExecSource::new(src, tgt_raw).with_opt_envelope(envelope);
    if rel.envelope.is_some() {
        rel = rel.with_output_transform(OutputTransformKind::JsonEnvelope);
    }
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn parse_rce_can_exec_relation(
    args: &[&str],
    ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("rce.can-exec effect expects exactly 2 args: source and target".to_string());
    }
    // The execution context may carry PROCEDURE_CMD — the grounded exploit
    // command that was just run to establish this RCE path.  Store it as the
    // wrapping envelope so subsequent commands through this hop re-invoke the
    // same exploit with the new command substituted for ${CMD}.
    let envelope = ctx
        .get("PROCEDURE_CMD")
        .filter(|v| !v.trim().is_empty())
        .cloned();
    let rel = RceCanExec::new(args[0], args[1]).with_opt_envelope(envelope);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn parse_container_escape_relation(
    args: &[&str],
    ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 1 {
        return Err(
            "container.escape effect expects exactly 1 arg: the source pod \
             (use `sys` or the pod entity ID; the node is resolved automatically)"
                .to_string(),
        );
    }

    // `sys` resolves to the entity the TTP executed on.
    let src = if args[0].eq_ignore_ascii_case("sys") {
        ctx.get("TARGET_ID")
            .filter(|id| !id.is_empty())
            .map(String::as_str)
            .ok_or_else(|| "container.escape: 'sys' requires TARGET_ID in context".to_string())?
    } else {
        args[0]
    };

    // Resolve the node entity ID. The pipeline injects TARGET_NODE_ID from
    // pod.node_name (if known) or from an existing runs-on graph edge.
    // If neither is available, the pod is running on an unknown node — create a
    // placeholder using the pod's short name as the best available guess.
    // The placeholder is aliased to the real node once its name is discovered
    // (e.g. via sys.node-name after running hostname on the host).
    let node_entity_id: String = ctx
        .get("TARGET_NODE_ID")
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
        .unwrap_or_else(|| {
            // Infer the node name from the pod's short name (the segment after
            // "/pod/" in the entity ID). Falls back to the last path component
            // for non-standard entity IDs.
            let pod_name = src
                .split("/pod/")
                .nth(1)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| src.rsplit('/').next().unwrap_or("unknown"));
            format!("node/escape_{}", pod_name)
        });

    // Parse the node name from `node/<name>` to construct the typed entity.
    let node_name = node_entity_id
        .strip_prefix("node/")
        .filter(|n| !n.is_empty())
        .ok_or_else(|| {
            format!(
                "container.escape: '{}' is not a valid node entity ID (expected node/<name>)",
                node_entity_id
            )
        })?
        .to_string();

    // Ensure the node entity exists. If a runs-on relation already references
    // this node, the campaign merges rather than duplicates it.
    // Mark the name authoritative when it came from pod.node_name (K8s API).
    let authoritative = ctx
        .get("TARGET_NODE_AUTHORITATIVE")
        .map(|v| v == "true")
        .unwrap_or(false);
    let mut node = K8sNode::new(&node_name);
    if authoritative {
        node.name_confidence = NameConfidence::Authoritative;
    }

    // PROCEDURE_CMD is the grounded escape command (e.g. `nsenter -t 1 ... ${CMD}`).
    // Store it as the envelope so subsequent commands through this hop are
    // wrapped with the same escape primitive.
    let envelope = ctx
        .get("PROCEDURE_CMD")
        .filter(|v| !v.trim().is_empty())
        .cloned();

    Ok(FactsUpdate {
        new_entities: vec![Box::new(node)],
        new_relations: vec![
            Box::new(RunsOn::new(src, &node_entity_id)),
            Box::new(ContainerEscape::new(src, &node_entity_id).with_opt_envelope(envelope)),
        ],
        entity_aliases: IndexSet::new(),
        ..Default::default()
    })
}

fn split_relation(effect: &str) -> Result<(&str, Vec<&str>), String> {
    let open = effect
        .find('(')
        .ok_or_else(|| format!("invalid relation effect: {}", effect))?;
    let close = effect
        .rfind(')')
        .ok_or_else(|| format!("invalid relation effect: {}", effect))?;

    if close <= open {
        return Err(format!("invalid relation effect: {}", effect));
    }

    let name = effect[..open].trim();
    let body = effect[open + 1..close].trim();

    let args = if body.is_empty() {
        Vec::new()
    } else {
        body.split(',').map(str::trim).collect()
    };

    Ok((name, args))
}

fn get_arg<'a>(args: &'a HashMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(v) = args.get(*key) {
            return Some(v.as_str());
        }
    }

    None
}

fn parse_bool_like(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "running"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ran_domain::{CanReach, KubeletExecSource, OutputTransformKind, SessionChannel};

    fn ctx() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn k8s_can_reach_creates_can_reach_relation() {
        let result = parse_effect("k8s.can-reach(pod/ns/a, pod/ns/b)", &ctx());
        let update = result.expect("should parse");
        assert_eq!(update.new_relations.len(), 1);
        let rel = update.new_relations[0].as_ref();
        assert!(rel.as_any().downcast_ref::<CanReach>().is_some());
        assert_eq!(rel.source_id().0, "pod/ns/a");
        assert_eq!(rel.target_id().0, "pod/ns/b");
        assert_eq!(rel.relation_name(), "can-reach");
    }

    #[test]
    fn k8s_can_reach_wrong_arg_count_returns_err() {
        assert!(parse_effect("k8s.can-reach(pod/ns/a)", &ctx()).is_err());
        assert!(parse_effect("k8s.can-reach(a, b, c)", &ctx()).is_err());
    }

    #[test]
    fn k8s_can_reach_relation_name_is_can_reach() {
        let update = parse_effect("k8s.can-reach(a, b)", &ctx()).unwrap();
        assert_eq!(update.new_relations[0].relation_name(), "can-reach");
    }

    #[test]
    fn k8s_can_reach_ids_containing_slashes_roundtrip_correctly() {
        let update = parse_effect(
            "k8s.can-reach(ns/default/pod/frontend, ns/default/pod/backend)",
            &ctx(),
        )
        .unwrap();
        let rel = &update.new_relations[0];
        assert_eq!(rel.source_id().0, "ns/default/pod/frontend");
        assert_eq!(rel.target_id().0, "ns/default/pod/backend");
    }

    #[test]
    fn c2_session_creates_session_channel_relation() {
        let update = parse_effect("c2.session(sliver, ns/default/pod/victim)", &ctx()).unwrap();
        assert_eq!(update.new_relations.len(), 1);
        let rel = update.new_relations[0]
            .as_any()
            .downcast_ref::<SessionChannel>()
            .expect("expected SessionChannel relation");
        assert_eq!(rel.source_id().0, "c2/sliver");
        assert_eq!(rel.target_id().0, "ns/default/pod/victim");
        assert_eq!(rel.session_id, "session/sliver");
    }

    #[test]
    fn c2_session_accepts_explicit_session_backend_id() {
        let update = parse_effect(
            "c2.session(session/sliver-operator-1, ns/default/pod/victim)",
            &ctx(),
        )
        .unwrap();
        let rel = update.new_relations[0]
            .as_any()
            .downcast_ref::<SessionChannel>()
            .expect("expected SessionChannel relation");
        assert_eq!(rel.source_id().0, "c2/sliver-operator-1");
        assert_eq!(rel.session_id, "session/sliver-operator-1");
    }

    #[test]
    fn c2_session_resolves_sys_target_from_context() {
        let mut args = ctx();
        args.insert("TARGET_ID".into(), "node/worker-1".into());

        let update = parse_effect("c2.session(sliver, sys)", &args).unwrap();
        let rel = update.new_relations[0]
            .as_any()
            .downcast_ref::<SessionChannel>()
            .expect("expected SessionChannel relation");
        assert_eq!(rel.target_id().0, "node/worker-1");
    }

    #[test]
    fn c2_session_wrong_arg_count_returns_err() {
        assert!(parse_effect("c2.session(sliver)", &ctx()).is_err());
        assert!(parse_effect("c2.session(a, b, c)", &ctx()).is_err());
    }

    #[test]
    fn kubelet_exec_source_with_sys_preserves_marker_and_metadata() {
        let mut args = ctx();
        args.insert("TARGET_ID".into(), "ns/default/pod/attacker".into());
        args.insert("PROCEDURE_CMD".into(), "ran-ws -- ${CMD}".into());

        let update = parse_effect("k8s.kubelet-exec-source(sys, all(k8s.node))", &args).unwrap();
        assert_eq!(update.new_relations.len(), 1);

        let rel = update.new_relations[0]
            .as_any()
            .downcast_ref::<KubeletExecSource>()
            .expect("expected KubeletExecSource relation");
        assert_eq!(rel.source_id().0, "ns/default/pod/attacker");
        assert_eq!(rel.target_id().0, "all(k8s.node)");
        assert_eq!(rel.envelope.as_deref(), Some("ran-ws -- ${CMD}"));
        assert_eq!(
            rel.output_transform,
            Some(OutputTransformKind::JsonEnvelope),
            "envelope-backed kubelet channel should request JSON unwrapping"
        );
    }

    #[test]
    fn kubelet_exec_source_without_envelope_has_no_output_transform() {
        let update = parse_effect(
            "k8s.kubelet-exec-source(ns/default/pod/a, all(k8s.node))",
            &ctx(),
        )
        .unwrap();
        let rel = update.new_relations[0]
            .as_any()
            .downcast_ref::<KubeletExecSource>()
            .expect("expected KubeletExecSource relation");
        assert!(rel.envelope.is_none());
        assert!(rel.output_transform.is_none());
    }

    // --- k8s.serviceaccount ---

    #[test]
    fn k8s_serviceaccount_creates_sa_entity() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("ServiceAccountName".into(), "my-sa".into());
        let update = parse_effect("k8s.serviceaccount", &args).unwrap();
        assert_eq!(update.new_entities.len(), 1);
        let sa = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::ServiceAccount>()
            .unwrap();
        assert_eq!(sa.entity_name(), "my-sa");
        assert_eq!(sa.namespace(), Some("default"));
    }

    #[test]
    fn k8s_serviceaccount_missing_namespace_returns_err() {
        let mut args = ctx();
        args.insert("ServiceAccountName".into(), "my-sa".into());
        assert!(parse_effect("k8s.serviceaccount", &args).is_err());
    }

    #[test]
    fn k8s_serviceaccount_missing_name_returns_err() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        assert!(parse_effect("k8s.serviceaccount", &args).is_err());
    }

    #[test]
    fn k8s_serviceaccount_optional_token_populates_sa_token() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("ServiceAccountName".into(), "my-sa".into());
        args.insert("Token".into(), "eyJhbGciOiJSUzI1NiJ9.test".into());
        let update = parse_effect("k8s.serviceaccount", &args).unwrap();
        let sa = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::ServiceAccount>()
            .unwrap();
        assert_eq!(sa.raw_token(), Some("eyJhbGciOiJSUzI1NiJ9.test"));
    }

    // --- k8s.role ---

    #[test]
    fn k8s_role_creates_role_with_namespace_and_name() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("RoleName".into(), "pod-reader".into());
        let update = parse_effect("k8s.role", &args).unwrap();
        let role = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sRole>()
            .unwrap();
        assert_eq!(role.entity_name(), "pod-reader");
        assert_eq!(role.namespace(), Some("default"));
        assert!(role.permissions.is_empty());
    }

    #[test]
    fn k8s_role_parses_rules_json() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("RoleName".into(), "pod-reader".into());
        args.insert(
            "Rules".into(),
            r#"[{"verbs":["get","list"],"resources":["pods"],"apiGroups":[""]}]"#.into(),
        );
        let update = parse_effect("k8s.role", &args).unwrap();
        let role = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sRole>()
            .unwrap();
        assert_eq!(role.permissions.len(), 2);
        assert!(role
            .permissions
            .iter()
            .any(|p| p.verb == "get" && p.resource_type == "pods"));
        assert!(role
            .permissions
            .iter()
            .any(|p| p.verb == "list" && p.resource_type == "pods"));
    }

    #[test]
    fn k8s_role_empty_rules_arg_creates_role_with_no_permissions() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("RoleName".into(), "empty-role".into());
        args.insert("Rules".into(), "[]".into());
        let update = parse_effect("k8s.role", &args).unwrap();
        let role = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sRole>()
            .unwrap();
        assert!(role.permissions.is_empty());
    }

    #[test]
    fn k8s_role_missing_name_returns_err() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        assert!(parse_effect("k8s.role", &args).is_err());
    }

    // --- k8s.rolebinding ---

    #[test]
    fn k8s_rolebinding_creates_binding_with_role_ref_and_subjects() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("BindingName".into(), "pod-reader-binding".into());
        args.insert("RoleRef".into(), "pod-reader".into());
        args.insert(
            "Subjects".into(),
            r#"[{"kind":"ServiceAccount","name":"my-sa","namespace":"default"}]"#.into(),
        );
        let update = parse_effect("k8s.rolebinding", &args).unwrap();
        let binding = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sRoleBinding>()
            .unwrap();
        assert_eq!(binding.entity_name(), "pod-reader-binding");
        assert_eq!(binding.role_ref, "pod-reader");
        assert_eq!(binding.subjects.len(), 1);
        assert_eq!(binding.subjects[0].name, "my-sa");
    }

    #[test]
    fn k8s_rolebinding_missing_binding_name_returns_err() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        assert!(parse_effect("k8s.rolebinding", &args).is_err());
    }

    #[test]
    fn k8s_rolebinding_empty_subjects_creates_binding() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("BindingName".into(), "empty-binding".into());
        args.insert("Subjects".into(), "[]".into());
        let update = parse_effect("k8s.rolebinding", &args).unwrap();
        let binding = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sRoleBinding>()
            .unwrap();
        assert!(binding.subjects.is_empty());
    }

    // --- k8s.cronjob ---

    #[test]
    fn k8s_cronjob_creates_cronjob_entity() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("CronJobName".into(), "cleanup-job".into());
        let update = parse_effect("k8s.cronjob", &args).unwrap();
        let cj = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::CronJob>()
            .unwrap();
        assert_eq!(cj.entity_name(), "cleanup-job");
        assert_eq!(cj.namespace(), Some("default"));
        assert!(cj.schedule.is_none());
    }

    #[test]
    fn k8s_cronjob_optional_schedule_arg_populated() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("CronJobName".into(), "nightly".into());
        args.insert("Schedule".into(), "0 2 * * *".into());
        let update = parse_effect("k8s.cronjob", &args).unwrap();
        let cj = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::CronJob>()
            .unwrap();
        assert_eq!(cj.schedule.as_deref(), Some("0 2 * * *"));
    }

    #[test]
    fn k8s_cronjob_missing_namespace_returns_err() {
        let mut args = ctx();
        args.insert("CronJobName".into(), "cleanup-job".into());
        assert!(parse_effect("k8s.cronjob", &args).is_err());
    }

    #[test]
    fn k8s_cronjob_missing_name_returns_err() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        assert!(parse_effect("k8s.cronjob", &args).is_err());
    }

    // --- container.escape ---

    #[test]
    fn container_escape_creates_node_and_relations_with_known_node() {
        let mut args = ctx();
        args.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
        args.insert(
            "PROCEDURE_CMD".into(),
            "nsenter -t 1 -m -u -i -n -p -- ${CMD}".into(),
        );
        let update = parse_effect("container.escape(ns/default/pod/attacker)", &args).unwrap();

        // Emits a K8sNode entity.
        assert_eq!(update.new_entities.len(), 1);
        let node = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sNode>()
            .unwrap();
        assert_eq!(node.entity_name(), "worker-1");

        // Emits RunsOn + ContainerEscape relations.
        assert_eq!(update.new_relations.len(), 2);
        let runs_on = update
            .new_relations
            .iter()
            .find(|r| r.relation_name() == "runs-on")
            .unwrap();
        assert_eq!(runs_on.source_id().0, "ns/default/pod/attacker");
        assert_eq!(runs_on.target_id().0, "node/worker-1");

        let escape = update
            .new_relations
            .iter()
            .find(|r| r.relation_name() == "container.escape")
            .unwrap();
        let escape = escape.as_any().downcast_ref::<ContainerEscape>().unwrap();
        assert_eq!(escape.source_id.0, "ns/default/pod/attacker");
        assert_eq!(escape.target_id.0, "node/worker-1");
        assert_eq!(
            escape.envelope.as_deref(),
            Some("nsenter -t 1 -m -u -i -n -p -- ${CMD}")
        );
        assert!(escape.is_exec_channel());
    }

    #[test]
    fn container_escape_node_is_authoritative_when_target_node_authoritative_flag_set() {
        let mut args = ctx();
        args.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
        args.insert("TARGET_NODE_AUTHORITATIVE".into(), "true".into());
        let update = parse_effect("container.escape(ns/default/pod/attacker)", &args).unwrap();
        let node = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sNode>()
            .unwrap();
        assert_eq!(node.name_confidence, NameConfidence::Authoritative);
    }

    #[test]
    fn container_escape_node_is_derived_without_authoritative_flag() {
        let mut args = ctx();
        args.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
        let update = parse_effect("container.escape(ns/default/pod/attacker)", &args).unwrap();
        let node = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sNode>()
            .unwrap();
        assert_eq!(node.name_confidence, NameConfidence::Derived);
    }

    #[test]
    fn container_escape_creates_placeholder_node_when_node_unknown() {
        let update = parse_effect("container.escape(ns/default/pod/attacker)", &ctx()).unwrap();

        // Should still create a node entity (placeholder) named after the pod's
        // short name — the best available guess before hostname is run.
        assert_eq!(update.new_entities.len(), 1);
        let node = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sNode>()
            .unwrap();
        assert_eq!(
            node.entity_name(),
            "escape_attacker",
            "placeholder should be escape_<pod-short-name>"
        );
        assert_eq!(node.entity_id().0, "node/escape_attacker");

        // And two relations: RunsOn + ContainerEscape both targeting the placeholder.
        assert_eq!(update.new_relations.len(), 2);
        assert!(update
            .new_relations
            .iter()
            .any(|r| r.relation_name() == "runs-on" && r.target_id().0 == "node/escape_attacker"));
        assert!(update
            .new_relations
            .iter()
            .any(|r| r.relation_name() == "container.escape"
                && r.target_id().0 == "node/escape_attacker"));
    }

    #[test]
    fn container_escape_sys_resolves_to_target_id() {
        let mut args = ctx();
        args.insert("TARGET_ID".into(), "ns/default/pod/attacker".into());
        args.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
        let update = parse_effect("container.escape(sys)", &args).unwrap();
        let escape = update
            .new_relations
            .iter()
            .find(|r| r.relation_name() == "container.escape")
            .unwrap();
        assert_eq!(escape.source_id().0, "ns/default/pod/attacker");
    }

    #[test]
    fn container_escape_without_procedure_cmd_has_no_envelope() {
        let mut args = ctx();
        args.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
        let update = parse_effect("container.escape(ns/default/pod/attacker)", &args).unwrap();
        let escape = update
            .new_relations
            .iter()
            .find(|r| r.relation_name() == "container.escape")
            .unwrap()
            .as_any()
            .downcast_ref::<ContainerEscape>()
            .unwrap();
        assert!(escape.envelope.is_none());
    }

    #[test]
    fn container_escape_wrong_arg_count_returns_err() {
        assert!(parse_effect("container.escape(a, b)", &ctx()).is_err());
        assert!(parse_effect("container.escape(a, b, c)", &ctx()).is_err());
        assert!(parse_effect("container.escape()", &ctx()).is_err());
    }

    // --- EffectKind taxonomy ---

    #[test]
    fn effect_kind_parse_strips_relation_args_and_normalizes_case() {
        assert_eq!(
            EffectKind::parse("c2.session(sliver, sys)"),
            Some(EffectKind::C2Session)
        );
        assert_eq!(EffectKind::parse("k8s.Pod"), Some(EffectKind::K8sPod));
        assert_eq!(EffectKind::parse("runs-on(a, b)"), Some(EffectKind::RunsOn));
        assert_eq!(EffectKind::parse("totally.unknown"), None);
    }

    #[test]
    fn every_dispatchable_effect_name_parses_to_a_kind() {
        // Every name the parser dispatches on must resolve through the taxonomy,
        // so the parser and the scorer can never see different vocabularies.
        for name in [
            "k8s.pod",
            "k8s.serviceaccount",
            "k8s.role",
            "k8s.rolebinding",
            "k8s.cronjob",
            "k8s.can-exec",
            "k8s.can-reach",
            "k8s.runs-on",
            "runs-on",
            "k8s.kubelet-exec-source",
            "k8s.kubelet-exec",
            "c2.session",
            "rce.can-exec",
            "container.escape",
        ] {
            let kind =
                EffectKind::parse(name).unwrap_or_else(|| panic!("no EffectKind for {name}"));
            // categories() is total — must classify every variant, non-empty.
            assert!(!kind.categories().is_empty());
        }
    }

    #[test]
    fn discovery_list_effects_classify_as_discovery() {
        for name in [
            "k8s.serviceAccountList",
            "k8s.secretList",
            "k8s.roleList",
            "k8s.SelfSubjectRulesReview",
            "rawServiceaccountToken",
            "sys.files",
            "file:content",
        ] {
            let kind = EffectKind::parse(name).unwrap_or_else(|| panic!("no kind for {name}"));
            assert!(
                kind.categories().contains(&EffectCategory::Discovery),
                "{name} should be Discovery"
            );
        }
    }

    #[test]
    fn volatility_split_stable_vs_volatile() {
        // Stable / idempotent facts.
        assert!(!EffectKind::parse("sys.ip").unwrap().is_volatile());
        assert!(!EffectKind::parse("rDNS").unwrap().is_volatile());
        assert!(!EffectKind::parse("container.escape(sys)")
            .unwrap()
            .is_volatile());
        // Volatile enumerations / mutable state.
        assert!(EffectKind::parse("k8s.podList").unwrap().is_volatile());
        assert!(EffectKind::parse("k8s.secretList").unwrap().is_volatile());
        assert!(EffectKind::parse("sys.processes").unwrap().is_volatile());
    }

    #[test]
    fn identity_facts_are_foundational_discovery() {
        // Environment facts learned alongside a token read (point b): they feed
        // grounding variables, so they're foundational discovery, never volatile.
        for name in ["Pod.name", "ServiceAccount.name", "Namespace.name"] {
            let k = EffectKind::parse(name).unwrap_or_else(|| panic!("no kind for {name}"));
            assert!(k.categories().contains(&EffectCategory::Discovery));
            assert_eq!(k.generality(), 1.0);
            assert!(!k.is_volatile());
        }
    }

    #[test]
    fn generality_tiers_rank_foundational_above_specialized() {
        let ip = EffectKind::parse("sys.ip").unwrap();
        let secrets = EffectKind::parse("k8s.secretList").unwrap();
        let mounts = EffectKind::parse("linux.mounts").unwrap();
        assert!(ip.generality() > secrets.generality());
        assert!(secrets.generality() > mounts.generality());
    }

    #[test]
    fn rbac_creation_effects_classify_as_privilege() {
        for name in ["create k8s.RoleBinding", "create k8s.Role"] {
            let kind = EffectKind::parse(name).unwrap_or_else(|| panic!("no kind for {name}"));
            assert!(
                kind.categories().contains(&EffectCategory::PrivilegeEdge),
                "{name} should be PrivilegeEdge"
            );
        }
    }

    #[test]
    fn network_and_adjacent_effects_classify_as_reachability() {
        for name in [
            "k8s.servicelist",
            "k8s.ingresslist",
            "k8s.gatewaylist",
            "k8s.httproutelist",
            "k8s.podList",
            "k8s.nodeList",
            "rDNS",
            "sys.ip",
        ] {
            let kind = EffectKind::parse(name).unwrap_or_else(|| panic!("no kind for {name}"));
            assert!(
                kind.categories().contains(&EffectCategory::Reachability),
                "{name} should be Reachability"
            );
            // network/adjacent enumeration is still information.
            assert!(kind.categories().contains(&EffectCategory::Discovery));
        }
    }
}
