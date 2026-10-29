use curve25519_dalek::scalar::Scalar;

use ech_db_protocol::crypto::{Curve, Hs};
use ech_db_protocol::id::Root;
use ech_db_protocol::value::{Admission, Submission};

pub struct Domain {
    root: Root,
    key: Scalar,
}

impl Domain {
    pub fn new(key: Scalar) -> Option<Self> {
        if key == Scalar::ZERO {
            return None;
        }
        let root = Root::new(Curve::encode(&(Curve::base() * key)))?;
        Some(Self { root, key })
    }

    pub fn generate() -> Option<Self> {
        Self::new(Self::random()?)
    }

    pub fn root(&self) -> Root {
        self.root
    }

    pub fn admit(&self, submission: Submission) -> Option<Admission> {
        let k = Self::random()?;
        let bytes = k.to_bytes();
        let payload = bcs::to_bytes(&(self.root, bytes, &submission)).ok()?;
        let challenge = Hs::scalar(b"admission", &payload);
        let sa = k + challenge * self.key;
        Some(Admission {
            k: bytes,
            sa: sa.to_bytes(),
        })
    }

    fn random() -> Option<Scalar> {
        let mut bytes = [0u8; 64];
        getrandom::fill(&mut bytes).ok()?;
        Some(Scalar::from_bytes_mod_order_wide(&bytes))
    }
}
