use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, thiserror::Error)]
pub enum ProtocolError {
    #[error("bcs: {0}")]
    Bcs(String),
    #[error("invalid hex encoding")]
    Hex,
    #[error("invalid root encoding")]
    Root,
}

impl From<bcs::Error> for ProtocolError {
    fn from(error: bcs::Error) -> Self {
        Self::Bcs(error.to_string())
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum Reject {
    UnknownNative { function: String },
    Application { bytes: Vec<u8> },
    BadRequest,
    BadSignature,
    Duplicate,
    NotFound,
    ProgramNotFound,
    NotAProgram,
    Abi,
    InvalidWasm,
    OutOfFuel,
    VmTrap,
    ResourceLimit,
    ReadOnly,
    Integrity,
    Unavailable,
}
