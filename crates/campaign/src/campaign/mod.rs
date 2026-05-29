mod entity_refs;
mod entity_store;
mod execution;
mod state;
#[cfg(test)]
mod tests;
mod types;

pub use entity_refs::{CampaignEntityRef, CampaignSystemEntityMut, CampaignSystemEntityRef};
pub use entity_store::{EntityStore, EntityType};
pub use state::Campaign;
pub use types::{
    ExecChannel, ExecuteActionError, ExecuteActionRequest, ExecuteActionResult,
    ExecutedActionEvent, TtpExecutionProcessing,
};
