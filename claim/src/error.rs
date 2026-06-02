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
    /// Substrate-level argument or shape error (e.g. unknown claim_type
    /// string at parse time). Domain-level schema validation lives in
    /// each consuming module and is the consumer's responsibility, not
    /// the substrate's.
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("missing genesis.amc at {0}")]
    MissingGenesis(String),
    #[error("corrupt {0}")]
    Corrupt(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
