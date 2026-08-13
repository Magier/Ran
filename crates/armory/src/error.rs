use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArmoryError {
    #[error("armory directory does not exist: {0}")]
    DirNotFound(String),
    #[error("failed to read armory file {path}: {source}")]
    ReadFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse TTP YAML in {path}: {source}")]
    ParseYaml {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("no TTPs loaded from armory directory: {0}")]
    NoTtpsLoaded(String),
    #[error("bundled armory file is not valid UTF-8: {0}")]
    InvalidUtf8(String),
    #[error("invalid TTP '{ttp_id}': {reason}")]
    InvalidTtp { ttp_id: String, reason: String },
}
