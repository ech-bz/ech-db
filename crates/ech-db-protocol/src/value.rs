use curve25519_dalek::RistrettoPoint;
use serde::{Deserialize, Serialize};

use crate::crypto::{Curve, Hs, Kdf};
use crate::error::{CodecError, Reject};
use crate::hash::Blake2b256;
use crate::id::{Root, Txid};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Identity {
    pub tweak: [u8; 32],
    pub nonce: [u8; 32],
    pub c1: [u8; 32],
    pub c2: [u8; 32],
}

impl Identity {
    pub fn derive(
        secret: &[u8; 32],
        root: &Root,
        tweak: [u8; 32],
        nonce: [u8; 32],
    ) -> Option<Self> {
        let anchor = Curve::point(root.as_array())?;
        let x = Kdf::scalar(secret, b"identity", root.as_bytes());
        let input = bcs::to_bytes(&(root, tweak)).ok()?;
        let r = Kdf::scalar(secret, b"encryption", &input);
        let c1 = Curve::base() * r;
        let c2 = Curve::base() * x + anchor * r;
        Some(Self {
            tweak,
            nonce,
            c1: Curve::encode(&c1),
            c2: Curve::encode(&c2),
        })
    }
}

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
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct UserProof {
    pub t1: [u8; 32],
    pub t2: [u8; 32],
    pub zx: [u8; 32],
    pub zr: [u8; 32],
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct User {
    pub identity: Identity,
    pub proof: UserProof,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Admission {
    pub k: [u8; 32],
    pub sa: [u8; 32],
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Tx {
    pub call: Call,
    pub user: Option<User>,
    pub admission: Admission,
}

impl Tx {
    pub fn txid(&self, root: &Root) -> Result<Txid, CodecError> {
        let identity = self.user.as_ref().map(|user| &user.identity);
        let bytes = bcs::to_bytes(&(root, &self.call, identity)).map_err(CodecError::from)?;
        Ok(Txid::new(Blake2b256::hash(&[bytes.as_slice()])))
    }

    pub fn verify(&self, root: &Root) -> Result<(), Reject> {
        let anchor = Curve::point(root.as_array()).ok_or(Reject::BadRequest)?;
        if let Some(user) = &self.user {
            self.verify_user(root, &anchor, user)?;
        }
        let k = Curve::point(&self.admission.k).ok_or(Reject::BadSignature)?;
        let sa = Curve::scalar(&self.admission.sa).ok_or(Reject::BadSignature)?;
        let payload = bcs::to_bytes(&(root, self.admission.k, &self.call, &self.user))
            .map_err(|_| Reject::BadRequest)?;
        let challenge = Hs::scalar(b"admission", &payload);
        if sa * Curve::base() != k + challenge * anchor {
            return Err(Reject::BadSignature);
        }
        Ok(())
    }

    fn verify_user(&self, root: &Root, anchor: &RistrettoPoint, user: &User) -> Result<(), Reject> {
        let base = Curve::base();
        let identity = &user.identity;
        let proof = &user.proof;
        let c1 = Curve::point(&identity.c1).ok_or(Reject::BadSignature)?;
        let c2 = Curve::point(&identity.c2).ok_or(Reject::BadSignature)?;
        if c1 == Curve::identity() {
            return Err(Reject::BadSignature);
        }
        let t1 = Curve::point(&proof.t1).ok_or(Reject::BadSignature)?;
        let t2 = Curve::point(&proof.t2).ok_or(Reject::BadSignature)?;
        let zx = Curve::scalar(&proof.zx).ok_or(Reject::BadSignature)?;
        let zr = Curve::scalar(&proof.zr).ok_or(Reject::BadSignature)?;
        let message = self
            .call
            .signing_bytes(root)
            .map_err(|_| Reject::BadRequest)?;
        let payload = bcs::to_bytes(&(
            root,
            identity.tweak,
            identity.nonce,
            identity.c1,
            identity.c2,
            proof.t1,
            proof.t2,
            message,
        ))
        .map_err(|_| Reject::BadRequest)?;
        let challenge = Hs::scalar(b"user", &payload);
        if zr * base != t1 + challenge * c1 {
            return Err(Reject::BadSignature);
        }
        if zx * base + zr * *anchor != t2 + challenge * c2 {
            return Err(Reject::BadSignature);
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Request {
    pub tx: Tx,
    pub sid: Option<[u8; 32]>,
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
