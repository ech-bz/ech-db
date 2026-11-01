use curve25519_dalek::scalar::Scalar;

use ech_db_protocol::crypto::Curve;
use ech_db_protocol::error::CodecError;
use ech_db_protocol::id::Root;
use ech_db_protocol::proof::{Proof, Statement};
use ech_db_protocol::value::{Decryption, Esid, ServerPart, Tx, UserPart};

use crate::Error;

pub struct Domain {
    root: Root,
    key: Scalar,
}

impl Domain {
    pub fn new(key: Scalar) -> Option<Self> {
        if key == Scalar::ZERO {
            return None;
        }
        let root = Root::new(Curve::encode(&(Curve::base() * key))).ok()?;
        Some(Self { root, key })
    }

    pub fn generate() -> Option<Self> {
        Self::new(Self::random().ok()?)
    }

    pub fn root(&self) -> Root {
        self.root
    }

    pub fn sign(&self, user: UserPart, ctx: Vec<u8>) -> Result<Tx, Error> {
        let statement = Statement::server(&self.root);
        let context = ServerPart::context(&self.root, &user, &ctx)?;
        let nonce = Self::random()?;
        let proof = Proof::prove(b"server", &statement, &context, &[self.key], &[nonce])
            .ok_or(Error::Proof)?;
        Ok(Tx {
            user,
            server: ServerPart { ctx, proof },
        })
    }

    pub fn decrypt(&self, esid: &Esid) -> Result<Decryption, Error> {
        let c1 = Curve::point(&esid.c1).ok_or(Error::Esid)?;
        let c2 = Curve::point(&esid.c2).ok_or(Error::Esid)?;
        let sid = Curve::encode(&(c2 - c1 * self.key));
        let statement = Statement::decryption(&self.root, esid, &sid).ok_or(Error::Proof)?;
        let context = bcs::to_bytes(&(self.root, esid, &sid)).map_err(CodecError::from)?;
        let nonce = Self::random()?;
        let proof = Proof::prove(b"decryption", &statement, &context, &[self.key], &[nonce])
            .ok_or(Error::Proof)?;
        Ok(Decryption { sid, proof })
    }

    fn random() -> Result<Scalar, Error> {
        let mut bytes = [0u8; 64];
        getrandom::fill(&mut bytes).map_err(|_| Error::Random)?;
        Ok(Scalar::from_bytes_mod_order_wide(&bytes))
    }
}
