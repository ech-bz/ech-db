use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("bcs: {0}")]
    Bcs(#[from] bcs::Error),
    #[error("invalid hex encoding")]
    Hex,
    #[error("invalid root encoding")]
    Root,
}

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
