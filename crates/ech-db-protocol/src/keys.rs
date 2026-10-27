use foundationdb_tuple::{pack, pack_with_versionstamp, Bytes, Versionstamp};
use serde::{Deserialize, Serialize};

use crate::id::{Root, Txid};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Namespace {
    Kv,
    Tx,
}

impl Namespace {
    pub fn name(self) -> &'static str {
        match self {
            Namespace::Kv => "kv",
            Namespace::Tx => "tx",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ReadKey {
    pub namespace: Namespace,
    pub suffix: Vec<u8>,
}

impl ReadKey {
    pub fn kv(user_key: Vec<u8>) -> Self {
        Self {
            namespace: Namespace::Kv,
            suffix: user_key,
        }
    }

    pub fn tx(suffix: Vec<u8>) -> Self {
        Self {
            namespace: Namespace::Tx,
            suffix,
        }
    }

    pub fn physical(&self, root: &Root) -> Vec<u8> {
        let mut key = Keys::namespace(root, self.namespace);
        key.extend_from_slice(&self.suffix);
        key
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Range {
    pub namespace: Namespace,
    pub start: Vec<u8>,
    pub end: Option<Vec<u8>>,
}

impl Range {
    pub fn kv(start: Vec<u8>, end: Option<Vec<u8>>) -> Self {
        Self {
            namespace: Namespace::Kv,
            start,
            end,
        }
    }

    pub fn tx(start: Vec<u8>, end: Option<Vec<u8>>) -> Self {
        Self {
            namespace: Namespace::Tx,
            start,
            end,
        }
    }

    pub fn physical(&self, root: &Root) -> (Vec<u8>, Vec<u8>) {
        let prefix = Keys::namespace(root, self.namespace);
        let mut begin = prefix.clone();
        begin.extend_from_slice(&self.start);
        let end = match &self.end {
            Some(end) => {
                let mut key = prefix;
                key.extend_from_slice(end);
                key
            }
            None => Prefix::successor(&prefix).expect("namespace prefix is not all 0xff"),
        };
        (begin, end)
    }
}

pub struct Prefix;

impl Prefix {
    pub fn successor(prefix: &[u8]) -> Option<Vec<u8>> {
        let mut key = prefix.to_vec();
        while let Some(last) = key.last_mut() {
            if *last < 0xff {
                *last += 1;
                return Some(key);
            }
            key.pop();
        }
        None
    }
}

pub struct Keys;

impl Keys {
    pub fn namespace(root: &Root, namespace: Namespace) -> Vec<u8> {
        pack(&(Bytes::from(root.as_bytes()), namespace.name()))
    }

    pub fn kv(root: &Root) -> Vec<u8> {
        Self::namespace(root, Namespace::Kv)
    }

    pub fn tx(root: &Root) -> Vec<u8> {
        Self::namespace(root, Namespace::Tx)
    }

    pub fn body(root: &Root, txid: &Txid, seq: u64) -> Vec<u8> {
        pack(&(
            Bytes::from(root.as_bytes()),
            "tx",
            Bytes::from(txid.as_bytes()),
            "body",
            seq,
        ))
    }

    pub fn body_prefix(root: &Root, txid: &Txid) -> Vec<u8> {
        pack(&(
            Bytes::from(root.as_bytes()),
            "tx",
            Bytes::from(txid.as_bytes()),
            "body",
        ))
    }

    pub fn body_len(root: &Root, txid: &Txid) -> Vec<u8> {
        pack(&(
            Bytes::from(root.as_bytes()),
            "tx",
            Bytes::from(txid.as_bytes()),
            "body_len",
        ))
    }

    pub fn response(root: &Root, txid: &Txid, seq: u64) -> Vec<u8> {
        pack(&(
            Bytes::from(root.as_bytes()),
            "tx",
            Bytes::from(txid.as_bytes()),
            "response",
            seq,
        ))
    }

    pub fn response_prefix(root: &Root, txid: &Txid) -> Vec<u8> {
        pack(&(
            Bytes::from(root.as_bytes()),
            "tx",
            Bytes::from(txid.as_bytes()),
            "response",
        ))
    }

    pub fn response_len(root: &Root, txid: &Txid) -> Vec<u8> {
        pack(&(
            Bytes::from(root.as_bytes()),
            "tx",
            Bytes::from(txid.as_bytes()),
            "response_len",
        ))
    }

    pub fn order(root: &Root) -> Vec<u8> {
        pack(&(Bytes::from(root.as_bytes()), "order"))
    }

    pub fn order_versionstamp_prefix(root: &Root) -> Vec<u8> {
        pack(&(Bytes::from(root.as_bytes()), "order", "versionstamp"))
    }

    pub fn order_versionstamp_next(root: &Root) -> Vec<u8> {
        pack_with_versionstamp(&(
            Bytes::from(root.as_bytes()),
            "order",
            "versionstamp",
            Versionstamp::incomplete(0),
        ))
    }

    pub fn order_versionstamp(root: &Root, versionstamp: [u8; 10]) -> Vec<u8> {
        pack(&(
            Bytes::from(root.as_bytes()),
            "order",
            "versionstamp",
            Versionstamp::complete(versionstamp, 0),
        ))
    }

    pub fn order_txid(root: &Root, txid: &Txid) -> Vec<u8> {
        pack(&(
            Bytes::from(root.as_bytes()),
            "order",
            "txid",
            Bytes::from(txid.as_bytes()),
        ))
    }
}
