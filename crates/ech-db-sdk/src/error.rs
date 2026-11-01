use ech_db_protocol::error::CodecError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("random source unavailable")]
    Random,
    #[error("codec: {0}")]
    Codec(#[from] CodecError),
    #[error("root is not a point")]
    Root,
    #[error("proof cannot be built")]
    Proof,
}
