use ech_db_protocol::error::ProtocolError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("random source unavailable")]
    Random,
    #[error("protocol: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("root is not a point")]
    Root,
    #[error("esid is not a point")]
    Esid,
    #[error("proof cannot be built")]
    Proof,
}
