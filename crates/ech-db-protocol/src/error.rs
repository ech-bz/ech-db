use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("bcs: {0}")]
    Bcs(#[from] bcs::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error(transparent)]
    Codec(#[from] CodecError),
}

#[derive(Debug, thiserror::Error)]
#[error("invalid hex encoding")]
pub struct HexError;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum Reject {
    BadRequest,
    BadSignature,
    Duplicate,
    NotFound,
    ProgramNotFound,
    NotAProgram,
    UnknownNative { function: String },
    Abi,
    InvalidWasm,
    OutOfFuel,
    VmTrap,
    ResourceLimit,
    ReadOnly,
    Application { bytes: Vec<u8> },
    Integrity,
    Unavailable,
}
