use blake2b_simd::Params;

pub struct Blake2b256(blake2b_simd::State);

impl Blake2b256 {
    pub fn start() -> Self {
        Self(Params::new().hash_length(32).to_state())
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub fn finalize(self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(self.0.finalize().as_bytes());
        out
    }

    pub fn hash(parts: &[&[u8]]) -> [u8; 32] {
        let mut hasher = Self::start();
        for part in parts {
            hasher.update(part);
        }
        hasher.finalize()
    }
}
