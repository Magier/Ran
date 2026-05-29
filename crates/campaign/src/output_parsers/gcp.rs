use std::collections::HashMap;

use ran_domain::{GCPBucket, GCPServiceAccount, GcpAccessToken};
use serde::Deserialize;

use super::ParserOutput;
use crate::FactsUpdate;

pub(super) fn register(m: &mut HashMap<&'static str, super::ParserFn>) {
    m.insert("gcp.serviceaccount", parse_gcp_serviceaccount);
    m.insert("gcp.buckets", parse_gcp_buckets);
}

// ---------------------------------------------------------------------------
// gcp.serviceaccount
// ---------------------------------------------------------------------------

/// Parser for the `gcp.serviceaccount` effect.
///
/// Expects JSON from `gcloud iam service-accounts describe --format=json` or
/// the GCP metadata server's service-account endpoint.  Two shapes are accepted:
///
/// ```json
/// {"email": "my-sa@project.iam.gserviceaccount.com"}
/// ```
/// or with an embedded access token:
/// ```json
/// {
///   "email": "my-sa@project.iam.gserviceaccount.com",
///   "token": {"access_token": "ya29.c...", "expires_in": 3599, "token_type": "Bearer"}
/// }
/// ```
///
/// The project is inferred from the email domain when it follows the standard
/// `<name>@<project>.iam.gserviceaccount.com` pattern.
fn parse_gcp_serviceaccount(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty gcp.serviceaccount output".to_string());
    }

    #[derive(Deserialize)]
    struct GcpSaJson {
        email: Option<String>,
        token: Option<GcpTokenJson>,
    }

    #[derive(Deserialize)]
    struct GcpTokenJson {
        access_token: Option<String>,
        expires_in: Option<i64>,
        token_type: Option<String>,
    }

    let parsed: GcpSaJson = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => {
            return ParserOutput::UnknownFormat(
                "failed to parse JSON from gcp.serviceaccount output".to_string(),
            )
        }
    };

    let email = parsed.email.unwrap_or_default();
    let mut sa = GCPServiceAccount::new(&email);

    // Infer project from standard GCP SA email patterns:
    //   <name>@<project>.iam.gserviceaccount.com
    //   <number>-compute@developer.gserviceaccount.com → project = <number>
    if let Some(at_pos) = email.find('@') {
        let domain = &email[at_pos + 1..];
        if let Some(project) = domain.strip_suffix(".iam.gserviceaccount.com") {
            sa.project = Some(project.to_string());
        } else if domain == "developer.gserviceaccount.com" {
            // Compute default SA: <project-number>-compute@developer...
            let local = &email[..at_pos];
            if let Some(project_num) = local.strip_suffix("-compute") {
                sa.project = Some(project_num.to_string());
            }
        }
    }

    if let Some(tok) = parsed.token {
        sa.token = Some(GcpAccessToken {
            access_token: tok.access_token.unwrap_or_default(),
            expires_in: tok.expires_in.unwrap_or(0),
            token_type: tok.token_type.unwrap_or_default(),
        });
    }

    let mut facts = FactsUpdate::default();
    facts.new_entities.push(Box::new(sa));

    ParserOutput::SuccessWithFacts(
        facts,
        format!(
            "discovered GCP service account: {}",
            if email.is_empty() { "default" } else { &email }
        ),
    )
}

// ---------------------------------------------------------------------------
// gcp.buckets
// ---------------------------------------------------------------------------

/// Parser for the `gcp.buckets` effect.
///
/// Expects JSON from `gsutil ls -Lb -j` or the GCS list-buckets API response:
/// ```json
/// {
///   "kind": "storage#buckets",
///   "items": [
///     {"id": "my-bucket", "name": "my-bucket", "location": "US-CENTRAL1"},
///     ...
///   ]
/// }
/// ```
fn parse_gcp_buckets(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty gcp.buckets output".to_string());
    }

    #[derive(Deserialize)]
    struct GcpBucketListJson {
        items: Option<Vec<GcpBucketJson>>,
    }

    #[derive(Deserialize)]
    struct GcpBucketJson {
        id: Option<String>,
        name: Option<String>,
        location: Option<String>,
    }

    let parsed: GcpBucketListJson = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => {
            return ParserOutput::UnknownFormat(
                "failed to parse JSON from gcp.buckets output".to_string(),
            )
        }
    };

    let items = parsed.items.unwrap_or_default();
    if items.is_empty() {
        return ParserOutput::KnownFailure("no GCP buckets found in output".to_string());
    }

    let mut facts = FactsUpdate::default();
    let mut count = 0usize;

    for item in items {
        let id = item.id.unwrap_or_default();
        let name = item.name.unwrap_or_else(|| id.clone());
        // Skip entries where both id and name are empty.
        if id.is_empty() && name.is_empty() {
            continue;
        }
        let bucket_id = if id.is_empty() { name.clone() } else { id };
        let mut bucket = GCPBucket::new(&bucket_id, &name);
        bucket.location = item.location;
        facts.new_entities.push(Box::new(bucket));
        count += 1;
    }

    if count == 0 {
        return ParserOutput::KnownFailure("no valid GCP buckets parsed".to_string());
    }

    ParserOutput::SuccessWithFacts(facts, format!("discovered {} GCP bucket(s)", count))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ran_domain::{Entity, GCPBucket, GCPServiceAccount};

    // --- gcp.serviceaccount ---

    #[test]
    fn parse_gcp_serviceaccount_valid_email() {
        let stdout = r#"{"email":"my-sa@my-project.iam.gserviceaccount.com"}"#;
        let result = parse_gcp_serviceaccount(stdout, "", &HashMap::new());
        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 1);
        let sa = facts.new_entities[0]
            .as_any()
            .downcast_ref::<GCPServiceAccount>()
            .unwrap();
        assert_eq!(sa.email, "my-sa@my-project.iam.gserviceaccount.com");
        assert_eq!(sa.project.as_deref(), Some("my-project"));
    }

    #[test]
    fn parse_gcp_serviceaccount_with_token() {
        let stdout = r#"{
            "email": "svc@proj.iam.gserviceaccount.com",
            "token": {"access_token": "ya29.tok", "expires_in": 3599, "token_type": "Bearer"}
        }"#;
        let ParserOutput::SuccessWithFacts(facts, _) = parse_gcp_serviceaccount(stdout, "", &HashMap::new()) else {
            panic!("expected SuccessWithFacts");
        };
        let sa = facts.new_entities[0]
            .as_any()
            .downcast_ref::<GCPServiceAccount>()
            .unwrap();
        let token = sa.token.as_ref().unwrap();
        assert_eq!(token.access_token, "ya29.tok");
        assert_eq!(token.expires_in, 3599);
        assert_eq!(token.token_type, "Bearer");
    }

    #[test]
    fn parse_gcp_serviceaccount_compute_default_sa() {
        let stdout = r#"{"email":"1234567890-compute@developer.gserviceaccount.com"}"#;
        let ParserOutput::SuccessWithFacts(facts, _) = parse_gcp_serviceaccount(stdout, "", &HashMap::new()) else {
            panic!("expected SuccessWithFacts");
        };
        let sa = facts.new_entities[0]
            .as_any()
            .downcast_ref::<GCPServiceAccount>()
            .unwrap();
        assert_eq!(sa.project.as_deref(), Some("1234567890"));
    }

    #[test]
    fn parse_gcp_serviceaccount_empty_json_object() {
        // Empty JSON {} is valid (email defaults to ""); entity created with empty email.
        let ParserOutput::SuccessWithFacts(facts, _) = parse_gcp_serviceaccount("{}", "", &HashMap::new()) else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 1);
        let sa = facts.new_entities[0]
            .as_any()
            .downcast_ref::<GCPServiceAccount>()
            .unwrap();
        assert_eq!(sa.entity_id().0, "gcp-sa/default");
    }

    #[test]
    fn parse_gcp_serviceaccount_empty_stdout_returns_known_failure() {
        assert!(matches!(
            parse_gcp_serviceaccount("", "", &HashMap::new()),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn parse_gcp_serviceaccount_invalid_json_returns_unknown_format() {
        assert!(matches!(
            parse_gcp_serviceaccount("{invalid}", "", &HashMap::new()),
            ParserOutput::UnknownFormat(_)
        ));
    }

    // --- gcp.buckets ---

    #[test]
    fn parse_gcp_buckets_valid_items() {
        let stdout = r#"{"kind":"storage#buckets","items":[
            {"id":"bucket1","name":"my-bucket-1","location":"US-CENTRAL1"},
            {"id":"bucket2","name":"my-bucket-2","location":"EU"}
        ]}"#;
        let ParserOutput::SuccessWithFacts(facts, detail) = parse_gcp_buckets(stdout, "", &HashMap::new()) else {
            panic!("expected SuccessWithFacts");
        };
        assert_eq!(facts.new_entities.len(), 2);
        assert!(detail.contains("2"));
        let b1 = facts.new_entities[0]
            .as_any()
            .downcast_ref::<GCPBucket>()
            .unwrap();
        assert_eq!(b1.name, "my-bucket-1");
        assert_eq!(b1.location.as_deref(), Some("US-CENTRAL1"));
        assert_eq!(b1.entity_id().0, "gcp/bucket/bucket1");
    }

    #[test]
    fn parse_gcp_buckets_empty_items_array_returns_known_failure() {
        let stdout = r#"{"kind":"storage#buckets","items":[]}"#;
        assert!(matches!(
            parse_gcp_buckets(stdout, "", &HashMap::new()),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn parse_gcp_buckets_missing_items_returns_known_failure() {
        let stdout = r#"{"kind":"storage#buckets"}"#;
        assert!(matches!(
            parse_gcp_buckets(stdout, "", &HashMap::new()),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn parse_gcp_buckets_empty_stdout_returns_known_failure() {
        assert!(matches!(
            parse_gcp_buckets("", "", &HashMap::new()),
            ParserOutput::KnownFailure(_)
        ));
    }

    #[test]
    fn parse_gcp_buckets_invalid_json_returns_unknown_format() {
        assert!(matches!(
            parse_gcp_buckets("{invalid}", "", &HashMap::new()),
            ParserOutput::UnknownFormat(_)
        ));
    }
}
