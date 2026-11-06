use foundationdb_tuple::{pack, pack_with_versionstamp, Bytes, Versionstamp};

use crate::id::{Root, Txid};

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
    pub fn kv(root: &Root) -> Vec<u8> {
        pack(&(Bytes::from(root.as_bytes()), "kv"))
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
