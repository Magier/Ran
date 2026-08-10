mod entity_refs;
mod entity_store;
pub(crate) mod execution;
mod state;
#[cfg(test)]
mod tests;
mod types;

pub use entity_refs::{CampaignEntityRef, CampaignSystemEntityMut, CampaignSystemEntityRef};
pub use entity_store::{EntityStore, EntityType};
pub use execution::best_tool_readiness;
pub use state::{Campaign, InitialClusterKnowledge, InitialKnowledge, InitialKubeconfigKnowledge};
pub use types::{
    ExecChannel, ExecuteActionError, ExecuteActionRequest, ExecuteActionResult,
    ExecutedActionEvent, TtpExecutionProcessing,
};
