pub trait Key: Sized {
    fn encode(&self) -> Vec<u8>;

    fn decode(bytes: &[u8]) -> Option<Self>;
}

impl Key for u64 {
    fn encode(&self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        Some(u64::from_be_bytes(bytes.try_into().ok()?))
    }
}

impl<const N: usize> Key for [u8; N] {
    fn encode(&self) -> Vec<u8> {
        self.to_vec()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok()
    }
}

impl Key for String {
    fn encode(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        String::from_utf8(bytes.to_vec()).ok()
    }
}

impl Key for ech_db_protocol::value::Esid {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(96);
        bytes.extend_from_slice(&self.tweak);
        bytes.extend_from_slice(&self.c1);
        bytes.extend_from_slice(&self.c2);
        bytes
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let bytes: [u8; 96] = bytes.try_into().ok()?;
        Some(ech_db_protocol::value::Esid {
            tweak: bytes[0..32].try_into().ok()?,
            c1: bytes[32..64].try_into().ok()?,
            c2: bytes[64..96].try_into().ok()?,
        })
    }
}

impl Key for () {
    fn encode(&self) -> Vec<u8> {
        Vec::new()
    }
    fn decode(bytes: &[u8]) -> Option<Self> {
        bytes.is_empty().then_some(())
    }
}
