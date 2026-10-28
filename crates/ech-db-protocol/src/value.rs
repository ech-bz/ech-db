use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::error::{CodecError, SignError};
use crate::hash::Blake2b256;
use crate::id::{Root, Txid};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Program {
    Txid(Txid),
    Native,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Call {
    pub program: Program,
    pub ctx: Vec<u8>,
    pub function: String,
    pub args: Vec<u8>,
}

impl Call {
    pub fn signing_bytes(&self, root: &Root) -> Result<Vec<u8>, CodecError> {
        bcs::to_bytes(&(root, self)).map_err(CodecError::from)
    }

    pub fn txid(&self, root: &Root) -> Result<Txid, CodecError> {
        let bytes = self.signing_bytes(root)?;
        Ok(Txid::new(Blake2b256::hash(&[bytes.as_slice()])))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Tx {
    pub call: Call,
    #[serde(with = "crate::fixed64")]
    pub signature: [u8; 64],
}

impl Tx {
    pub fn sign(root: &Root, call: Call, key: &SigningKey) -> Result<Self, SignError> {
        let bytes = call.signing_bytes(root)?;
        Ok(Self {
            signature: key.sign(&bytes).to_bytes(),
            call,
        })
    }

    pub fn verify(&self, root: &Root) -> bool {
        let Ok(bytes) = self.call.signing_bytes(root) else {
            return false;
        };
        let Ok(key) = VerifyingKey::from_bytes(root.as_array()) else {
            return false;
        };
        let signature = Signature::from_bytes(&self.signature);
        key.verify_strict(&bytes, &signature).is_ok()
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Invocation {
    pub root: Root,
    pub call: Call,
    pub private: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Request {
    pub tx: Tx,
    pub private: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub txid: Txid,
    pub result: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Record {
    pub tx: Tx,
    pub result: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum CallOutput {
    Ok(Vec<u8>),
    Err(Vec<u8>),
}

pub struct Chunks;

impl Chunks {
    pub const SIZE: usize = 50 * 1024;
    pub const MAX_BODY: usize = 4 * 1024 * 1024;
    pub const MAX_RESULT: usize = 4 * 1024 * 1024;

    pub fn split(bytes: &[u8]) -> Vec<&[u8]> {
        bytes.chunks(Self::SIZE).collect()
    }

    pub fn count(bytes: &[u8]) -> u64 {
        bytes.len().div_ceil(Self::SIZE) as u64
    }

    pub fn limit_for(len: usize) -> u64 {
        (len / Self::SIZE + 2) as u64
    }
}
