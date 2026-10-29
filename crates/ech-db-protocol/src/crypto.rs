use blake2b_simd::Params;
use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use curve25519_dalek::ristretto::{CompressedRistretto, RistrettoPoint};
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::traits::Identity;

pub struct Curve;

impl Curve {
    const WIDE: usize = 64;

    pub fn base() -> RistrettoPoint {
        RISTRETTO_BASEPOINT_POINT
    }

    pub fn identity() -> RistrettoPoint {
        RistrettoPoint::identity()
    }

    pub fn point(bytes: &[u8; 32]) -> Option<RistrettoPoint> {
        CompressedRistretto::from_slice(bytes).ok()?.decompress()
    }

    pub fn scalar(bytes: &[u8; 32]) -> Option<Scalar> {
        Scalar::from_canonical_bytes(*bytes).into_option()
    }

    pub fn encode(point: &RistrettoPoint) -> [u8; 32] {
        point.compress().to_bytes()
    }

    pub fn reduce(bytes: &[u8]) -> Scalar {
        let mut wide = [0u8; Self::WIDE];
        wide.copy_from_slice(bytes);
        Scalar::from_bytes_mod_order_wide(&wide)
    }
}

pub struct Hs;

impl Hs {
    pub fn scalar(domain: &[u8], payload: &[u8]) -> Scalar {
        let mut state = Params::new().hash_length(Curve::WIDE).to_state();
        state.update(domain);
        state.update(payload);
        Curve::reduce(state.finalize().as_bytes())
    }
}
