use serde::Serialize;

use ech_db_protocol::error::ProtocolError;

#[derive(Debug, Serialize, thiserror::Error)]
pub enum Error {
    #[error("protocol: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("random source unavailable")]
    Random,
    #[error("root is not a point")]
    Root,
    #[error("esid is not a point")]
    Esid,
    #[error("proof cannot be built")]
    Proof,
}
