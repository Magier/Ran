use serde::{Deserialize, Serialize};

/// A JSON Web Token as extracted from a running pod.
///
/// Equivalent to Go's `JWToken` but with idiomatic snake_case and `Option`
/// for fields that may be absent rather than zero-value strings/ints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JwToken {
    pub subject: Option<String>,
    pub audience: Vec<String>,
    pub issuer: Option<String>,
    pub expires_at: Option<i64>,
    pub issued_at: Option<i64>,
    /// The raw encoded JWT string.
    pub raw: String,
}

impl JwToken {
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
}

/// A Kubernetes ServiceAccount token with its in-cluster claims.
///
/// Replaces Go's `ServiceAccountToken` (which embedded `JWToken` and had the
/// Kubernetes claims in an anonymous inner struct). The claims are flattened
/// here for clarity.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceAccountToken {
    pub jwt: JwToken,
    pub namespace: String,
    /// Name of the pod this token was extracted from, if known.
    pub pod_name: Option<String>,
    pub pod_uid: Option<String>,
    pub service_account_name: String,
    pub service_account_uid: Option<String>,
    /// Whether the token was actually mounted/seen vs just known to exist.
    pub is_bound: bool,
}

impl ServiceAccountToken {
    /// The raw JWT string, empty if not yet extracted.
    pub fn raw(&self) -> &str {
        &self.jwt.raw
    }

    pub fn has_token(&self) -> bool {
        !self.jwt.is_empty()
    }
}
