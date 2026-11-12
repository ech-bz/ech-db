use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, thiserror::Error)]
pub enum GuestError {
    #[error("unbound collection")]
    Unbound,
    #[error("invalid collection key")]
    InvalidKey,
    #[error("collection limit overflow")]
    Limit,
    #[error("invalid collection batch")]
    Batch,
    #[error("bcs: {0}")]
    Bcs(String),
}

impl From<bcs::Error> for GuestError {
    fn from(error: bcs::Error) -> Self {
        Self::Bcs(error.to_string())
    }
}
