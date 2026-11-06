use ech_db_protocol::{
    hash::Blake2b256,
    id::Root,
    keys::{Keys, Prefix},
    map::{MapKey, MapRange},
};

pub(crate) struct BigMap {
    prefix: Vec<u8>,
}

impl BigMap {
    pub fn open(root: &Root, name: &[u8]) -> Self {
        let mut prefix = Keys::kv(root);
        prefix.extend_from_slice(&Blake2b256::hash(&[b"ech/big-map", name]));
        Self { prefix }
    }

    pub fn key(&self, key: &[u8]) -> Vec<u8> {
        let mut bytes = self.prefix.clone();
        bytes.extend_from_slice(key);
        bytes
    }

    pub fn physical(root: &Root, key: &MapKey) -> Vec<u8> {
        Self::open(root, &key.map).key(&key.key)
    }

    pub fn bounds(&self, range: &MapRange) -> (Vec<u8>, Vec<u8>) {
        let begin = match &range.after {
            Some(key) => {
                let mut begin = self.key(key);
                begin.push(0);
                begin
            }
            None => self.prefix.clone(),
        };
        let end = match &range.before {
            Some(key) => self.key(key),
            None => Prefix::successor(&self.prefix).expect("map prefix has a successor"),
        };
        (begin, end)
    }

    pub fn logical<'a>(&self, key: &'a [u8]) -> Option<&'a [u8]> {
        key.strip_prefix(self.prefix.as_slice())
    }
}
