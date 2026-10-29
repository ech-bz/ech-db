use std::fmt;

use blake2b_simd::Params;
use curve25519_dalek::scalar::Scalar;

use ech_db_protocol::crypto::{Curve, Hs};
use ech_db_protocol::id::Root;
use ech_db_protocol::value::{Call, Commitment, Esid, Response, Submission, User, UserProof};

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

    pub fn esid(&self, root: &Root, tweak: [u8; 32]) -> Option<Esid> {
        let anchor = Curve::point(root.as_array())?;
        let (x, r) = self.scalars(root, tweak)?;
        let c1 = Curve::base() * r;
        let c2 = Curve::base() * x + anchor * r;
        Some(Esid {
            tweak,
            c1: Curve::encode(&c1),
            c2: Curve::encode(&c2),
        })
    }

    pub fn submit(&self, root: &Root, call: Call, tweak: [u8; 32]) -> Option<Submission> {
        let anchor = Curve::point(root.as_array())?;
        let (x, r) = self.scalars(root, tweak)?;
        let esid = self.esid(root, tweak)?;
        let beta = Self::random()?;
        let alpha = Self::random()?;
        let t1 = Curve::base() * beta;
        let t2 = Curve::base() * alpha + anchor * beta;
        let commitment = Commitment {
            t1: Curve::encode(&t1),
            t2: Curve::encode(&t2),
        };
        let message = call.signing_bytes(root).ok()?;
        let payload = bcs::to_bytes(&(root, &esid, &commitment, message)).ok()?;
        let challenge = Hs::scalar(b"user", &payload);
        let zx = alpha + challenge * x;
        let zr = beta + challenge * r;
        Some(Submission {
            call,
            user: Some(User {
                esid,
                proof: UserProof {
                    commitment,
                    response: Response {
                        zx: zx.to_bytes(),
                        zr: zr.to_bytes(),
                    },
                },
            }),
        })
    }

    fn scalars(&self, root: &Root, tweak: [u8; 32]) -> Option<(Scalar, Scalar)> {
        let x = Self::derive(&self.0, b"identity", root.as_bytes());
        let info = bcs::to_bytes(&(root, tweak)).ok()?;
        let r = Self::derive(&self.0, b"encryption", &info);
        Some((x, r))
    }

    fn derive(secret: &[u8; 32], domain: &[u8], info: &[u8]) -> Scalar {
        let mut state = Params::new().hash_length(64).key(secret).to_state();
        state.update(domain);
        state.update(info);
        Curve::reduce(state.finalize().as_bytes())
    }

    fn random() -> Option<Scalar> {
        let mut bytes = [0u8; 64];
        getrandom::fill(&mut bytes).ok()?;
        Some(Scalar::from_bytes_mod_order_wide(&bytes))
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Client(..)")
    }
}
