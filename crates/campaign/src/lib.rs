#[cfg(test)]
mod analyzers;
mod campaign;
pub mod effects;
pub mod execution_record;
pub mod external_parser;
pub mod failure_analyzers;
pub mod grounding;
pub mod output_parsers;
pub mod rules;
pub mod runtime;
pub mod ttp_applicability;
pub use campaign::{
    Campaign, CampaignEntityRef, CampaignSystemEntityMut, CampaignSystemEntityRef,
    ExecuteActionError, ExecuteActionRequest, ExecuteActionResult, ExecutedActionEvent,
    TtpExecutionProcessing,
};
pub use c2::ExecTtp;
pub use effects::FactsUpdate;
pub use execution_record::ExecutionRecord;
pub use external_parser::{ExternalParseRequest, ExternalParseResponse, ExternalParser};
pub use output_parsers::{ParseAudit, ParseResult};
pub use rules::{default_rules, run_rules_fixpoint, InferenceRule, RuleTrigger};
pub use runtime::{spawn_c2_event_processor, spawn_c2_event_processor_with_external_parser, CampaignEvent, CampaignEventBus, EntitySummary};
