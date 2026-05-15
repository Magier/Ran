#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("plan parse error: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("plan validation error: {0}")]
    Validation(String),
    #[error("unknown step reference '{0}' in depends_on")]
    UnknownStepRef(String),
    #[error("circular dependency detected involving step '{0}'")]
    CircularDependency(String),
}
