use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("automerge: {0}")]
    Automerge(#[from] automerge::AutomergeError),
    #[error("automerge load: {0}")]
    AutomergeLoad(#[from] automerge::LoadChangeError),
    #[error("serde_json: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("toml de: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("toml ser: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("crypto: {0}")]
    Crypto(String),
    /// Substrate-level argument or shape error (e.g. empty session id,
    /// unknown claim_type string at parse time). Domain-level schema
    /// validation lives in [`crate::validation::SchemaError`] and is
    /// the consumer's responsibility, not the substrate's.
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("missing genesis.amc at {0}")]
    MissingGenesis(String),
    #[error("key file not found at {0}")]
    KeyFileNotFound(String),
    #[error("corrupt {0}")]
    Corrupt(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
