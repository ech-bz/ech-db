use foundationdb::FdbError;

use ech_db_protocol::error::Reject;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("rejected: {0:?}")]
    Reject(Reject),
    #[error("fdb: {0}")]
    Fdb(#[from] FdbError),
}

impl DbError {
    pub fn reject(reject: Reject) -> Self {
        Self::Reject(reject)
    }

    pub fn integrity() -> Self {
        Self::Reject(Reject::Integrity)
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::Fdb(error) if error.is_retryable())
    }
}
