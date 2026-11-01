use std::fmt;

use blake2b_simd::Params;
use curve25519_dalek::scalar::Scalar;

use ech_db_protocol::crypto::Curve;
use ech_db_protocol::error::CodecError;
use ech_db_protocol::id::Root;
use ech_db_protocol::proof::{Proof, Statement};
use ech_db_protocol::value::{Author, Call, Esid, UserPart};

use crate::Error;

pub struct Client([u8; 32]);

impl Client {
    pub fn new(bytes: [u8; 32]) -> Option<Self> {
        if bytes.iter().all(|byte| *byte == 0) {
            return None;
        }
        Some(Self(bytes))
    }

    pub fn generate() -> Option<Self> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).ok()?;
        Self::new(bytes)
    }

    pub fn esid(&self, root: &Root, tweak: [u8; 32]) -> Result<Esid, Error> {
        let anchor = Curve::point(root.as_array()).ok_or(Error::Root)?;
        let (x, r) = self.scalars(root, tweak)?;
        let c1 = Curve::base() * r;
        let c2 = Curve::base() * x + anchor * r;
        Ok(Esid {
            tweak,
            c1: Curve::encode(&c1),
            c2: Curve::encode(&c2),
        })
    }

    pub fn sign(
        &self,
        root: &Root,
        ctx: Vec<u8>,
        call: Call,
        tweak: [u8; 32],
    ) -> Result<UserPart, Error> {
        let (x, r) = self.scalars(root, tweak)?;
        let esid = self.esid(root, tweak)?;
        let statement = Statement::user(root, &esid);
        let context = UserPart::context(root, &ctx, &call)?;
        let alpha = Self::random()?;
        let beta = Self::random()?;
        let proof = Proof::prove(b"user", &statement, &context, &[x, r], &[alpha, beta])
            .ok_or(Error::Proof)?;
        Ok(UserPart {
            ctx,
            call,
            author: Author::User { esid, proof },
        })
    }

    fn scalars(&self, root: &Root, tweak: [u8; 32]) -> Result<(Scalar, Scalar), Error> {
        let x = Self::derive(&self.0, b"identity", root.as_bytes());
        let info = bcs::to_bytes(&(root, tweak)).map_err(CodecError::from)?;
        let r = Self::derive(&self.0, b"encryption", &info);
        Ok((x, r))
    }

    fn derive(secret: &[u8; 32], domain: &[u8], info: &[u8]) -> Scalar {
        let mut state = Params::new().hash_length(64).key(secret).to_state();
        state.update(domain);
        state.update(info);
        Curve::reduce(state.finalize().as_bytes())
    }

    fn random() -> Result<Scalar, Error> {
        let mut bytes = [0u8; 64];
        getrandom::fill(&mut bytes).map_err(|_| Error::Random)?;
        Ok(Scalar::from_bytes_mod_order_wide(&bytes))
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Client(..)")
    }
}
