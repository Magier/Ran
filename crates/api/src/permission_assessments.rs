//! Ran-owned permission risk assessments.
//!
//! This registry is deliberately separate from the KubeTier integration.
//! Entries here express Ran's reviewed product-specific assessment and take
//! precedence in the UI over a matching third-party assessment.

use crate::LocalPermissionAssessment;

/// Return the reviewed permission assessments shipped with Ran.
pub(crate) fn all() -> Vec<LocalPermissionAssessment> {
    vec![LocalPermissionAssessment {
        verb: "create".to_string(),
        resource: "servicemonitors".to_string(),
        api_group: "monitoring.coreos.com".to_string(),
        tier: "T2".to_string(),
        scope: Some("namespaced".to_string()),
        description: Some(
            "Can create Prometheus scrape configurations that reference bearer-token files."
                .to_string(),
        ),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_the_servicemonitor_assessment() {
        assert!(all().iter().any(|assessment| {
            assessment.verb == "create"
                && assessment.resource == "servicemonitors"
                && assessment.api_group == "monitoring.coreos.com"
                && assessment.tier == "T2"
        }));
    }
}
