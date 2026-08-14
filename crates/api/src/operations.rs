use campaign::ttp_applicability::{resolve_target_context, ttp_applicable_for_target};

use crate::{ApiError, ApiService, GetArmoryParams};

pub(crate) enum ApplicableTtpsError {
    Api(ApiError),
    UnknownTarget(String),
}

impl From<ApiError> for ApplicableTtpsError {
    fn from(error: ApiError) -> Self {
        Self::Api(error)
    }
}

/// Return the enabled TTPs for discovery, or the applicable TTPs for a target.
///
/// Campaign and armory retrieval live here so HTTP and MCP expose the same
/// selection rules without duplicating application logic.
pub(crate) async fn applicable_ttps<S: ApiService>(
    service: &S,
    target_id: Option<&str>,
) -> Result<Vec<armory::Ttp>, ApplicableTtpsError> {
    let all_ttps = service.get_armory(GetArmoryParams { tactic: None }).await?;
    let target_id = target_id.map(str::trim).unwrap_or_default();

    if target_id.is_empty() {
        return Ok(enabled_ttps(all_ttps));
    }

    let campaign = service.get_campaign().await?;
    applicable_ttps_for_target(all_ttps, &campaign, target_id)
}

fn enabled_ttps(ttps: Vec<armory::Ttp>) -> Vec<armory::Ttp> {
    ttps.into_iter()
        .filter(|ttp| !ttp.status.eq_ignore_ascii_case("disabled"))
        .collect()
}

fn applicable_ttps_for_target(
    ttps: Vec<armory::Ttp>,
    campaign: &campaign::Campaign,
    target_id: &str,
) -> Result<Vec<armory::Ttp>, ApplicableTtpsError> {
    let target = resolve_target_context(campaign, target_id)
        .ok_or_else(|| ApplicableTtpsError::UnknownTarget(target_id.to_owned()))?;

    Ok(ttps
        .into_iter()
        .filter(|ttp| ttp_applicable_for_target(ttp, campaign, &target))
        .collect())
}

#[cfg(test)]
mod tests {
    use ran_domain::K8sCluster;

    use super::*;

    #[test]
    fn discovery_excludes_disabled_ttps_case_insensitively() {
        let stable = armory::Ttp::new("stable", "Stable", "Discovery");
        let mut disabled = armory::Ttp::new("disabled", "Disabled", "Discovery");
        disabled.status = "DISABLED".to_string();

        let selected = enabled_ttps(vec![stable, disabled]);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "stable");
    }

    #[test]
    fn target_selection_reports_the_unknown_target() {
        let campaign = campaign::Campaign::bootstrap("Ran", K8sCluster::new("dev"));

        let result = applicable_ttps_for_target(Vec::new(), &campaign, "missing");

        match result {
            Err(ApplicableTtpsError::UnknownTarget(target_id)) => {
                assert_eq!(target_id, "missing");
            }
            _ => panic!("expected an unknown-target error"),
        }
    }
}
