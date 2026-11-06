use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::crypto::Curve;
use crate::error::{CodecError, Reject};
use crate::hash::Blake2b256;
use crate::id::{Root, Txid};
use crate::proof::{Equation, Proof, Statement, Term};

#[derive(
    crate::Bind, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug,
)]
#[bind(crate = "crate")]
pub struct Esid {
    pub tweak: [u8; 32],
    pub c1: [u8; 32],
    pub c2: [u8; 32],
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Program {
    Txid(Txid),
    Native,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Call {
    pub program: Program,
    pub function: String,
    #[serde(with = "serde_bytes")]
    pub args: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum Author {
    User { esid: Esid, proof: Proof },
    Server,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct UserPart {
    #[serde(with = "serde_bytes")]
    pub ctx: Vec<u8>,
    pub call: Call,
    pub author: Author,
}

impl UserPart {
    pub fn context(root: &Root, ctx: &[u8], call: &Call) -> Result<Vec<u8>, CodecError> {
        bcs::to_bytes(&(root, ctx, call)).map_err(CodecError::from)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ServerPart {
    #[serde(with = "serde_bytes")]
    pub ctx: Vec<u8>,
    pub proof: Proof,
}

impl ServerPart {
    pub fn context(root: &Root, user: &UserPart, ctx: &[u8]) -> Result<Vec<u8>, CodecError> {
        bcs::to_bytes(&(root, user, ctx)).map_err(CodecError::from)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Tx {
    pub user: UserPart,
    pub server: ServerPart,
}

impl Tx {
    pub fn txid(&self, root: &Root) -> Result<Txid, CodecError> {
        let bytes = bcs::to_bytes(&(root, &self.user)).map_err(CodecError::from)?;
        Ok(Txid::new(Blake2b256::hash(&[bytes.as_slice()])))
    }

    pub fn verify(&self, root: &Root) -> Result<(), Reject> {
        if let Author::User { esid, proof } = &self.user.author {
            if Curve::point(&esid.c1) == Some(Curve::identity()) {
                return Err(Reject::BadSignature);
            }
            let statement = Statement::user(root, esid);
            let context = UserPart::context(root, &self.user.ctx, &self.user.call)
                .map_err(|_| Reject::BadRequest)?;
            proof.verify(b"user", &statement, &context)?;
        }
        let statement = Statement::server(root);
        let context = ServerPart::context(root, &self.user, &self.server.ctx)
            .map_err(|_| Reject::BadRequest)?;
        self.server.proof.verify(b"server", &statement, &context)
    }
}

impl Statement {
    pub fn user(root: &Root, esid: &Esid) -> Self {
        let base = Curve::encode(&Curve::base());
        Self {
            equations: vec![
                Equation {
                    terms: vec![Term { base, response: 1 }],
                    target: esid.c1,
                },
                Equation {
                    terms: vec![
                        Term { base, response: 0 },
                        Term {
                            base: *root.as_array(),
                            response: 1,
                        },
                    ],
                    target: esid.c2,
                },
            ],
        }
    }

    pub fn server(root: &Root) -> Self {
        Self {
            equations: vec![Equation {
                terms: vec![Term {
                    base: Curve::encode(&Curve::base()),
                    response: 0,
                }],
                target: *root.as_array(),
            }],
        }
    }

    pub fn decryption(root: &Root, esid: &Esid, sid: &[u8; 32]) -> Option<Self> {
        let c1 = Curve::point(&esid.c1)?;
        let c2 = Curve::point(&esid.c2)?;
        let sid = Curve::point(sid)?;
        let mut statement = Self::server(root);
        statement.equations.push(Equation {
            terms: vec![Term {
                base: Curve::encode(&-c1),
                response: 0,
            }],
            target: Curve::encode(&(sid - c2)),
        });
        Some(statement)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Decryption {
    pub sid: [u8; 32],
    pub proof: Proof,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Request {
    pub tx: Tx,
    pub decryptions: BTreeMap<Esid, Decryption>,
}

impl Request {
    pub fn verify(&self, root: &Root) -> Result<(), Reject> {
        self.tx.verify(root)?;
        for (esid, decryption) in &self.decryptions {
            let statement =
                Statement::decryption(root, esid, &decryption.sid).ok_or(Reject::BadSignature)?;
            let context =
                bcs::to_bytes(&(root, esid, &decryption.sid)).map_err(|_| Reject::BadRequest)?;
            decryption
                .proof
                .verify(b"decryption", &statement, &context)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub txid: Txid,
    #[serde(with = "serde_bytes")]
    pub result: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Record {
    pub tx: Tx,
    #[serde(with = "serde_bytes")]
    pub result: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum CallOutput {
    Ok(#[serde(with = "serde_bytes")] Vec<u8>),
    Err(#[serde(with = "serde_bytes")] Vec<u8>),
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
