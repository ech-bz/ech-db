use bcs::from_bytes;

use ech_db_protocol::error::Reject;
use ech_db_protocol::id::{Root, Txid};
use ech_db_protocol::keys::{Keys, Prefix};
use ech_db_protocol::value::{Chunks, Record, Tx};

use crate::error::DbError;
use crate::fdb::{Edit, Reader};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Forward,
    Backward,
}

impl Dir {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "forward" => Some(Dir::Forward),
            "backward" => Some(Dir::Backward),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct History {
    pub dir: Dir,
    pub from: Option<Txid>,
    pub limit: u64,
}

pub struct Log;

impl Log {
    pub async fn exists<R: Reader>(access: &R, root: &Root, txid: &Txid) -> Result<bool, DbError> {
        Ok(access.get(&Keys::order_txid(root, txid)).await?.is_some())
    }

    pub async fn tx<R: Reader>(access: &R, root: &Root, txid: &Txid) -> Result<Tx, DbError> {
        let body = Self::body(access, root, txid).await?;
        let tx: Tx = from_bytes(&body).map_err(|_| DbError::integrity())?;
        let computed = tx.call.txid(root).map_err(|_| DbError::integrity())?;
        if computed != *txid {
            return Err(DbError::integrity());
        }
        Ok(tx)
    }

    pub async fn body<R: Reader>(access: &R, root: &Root, txid: &Txid) -> Result<Vec<u8>, DbError> {
        Self::part(access, root, txid, Part::Body).await
    }

    pub async fn response<R: Reader>(
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<Vec<u8>, DbError> {
        Self::part(access, root, txid, Part::Response).await
    }

    pub async fn record<R: Reader>(
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<Record, DbError> {
        let tx = Self::tx(access, root, txid).await?;
        let result = Self::response(access, root, txid).await?;
        Ok(Record { tx, result })
    }

    pub async fn page<R: Reader>(
        access: &R,
        root: &Root,
        history: &History,
    ) -> Result<Vec<Record>, DbError> {
        let prefix = Keys::order_versionstamp_prefix(root);
        let end = Prefix::successor(&prefix).ok_or_else(DbError::integrity)?;
        let rows = match (history.dir, history.from) {
            (Dir::Forward, Some(txid)) => {
                let begin =
                    Keys::order_versionstamp(root, Self::versionstamp(access, root, &txid).await?);
                access.scan(&begin, &end, Some(history.limit)).await?
            }
            (Dir::Forward, None) => access.scan(&prefix, &end, Some(history.limit)).await?,
            (Dir::Backward, Some(txid)) => {
                let mut after =
                    Keys::order_versionstamp(root, Self::versionstamp(access, root, &txid).await?);
                after.push(0);
                access
                    .scan_reverse(&prefix, &after, Some(history.limit))
                    .await?
            }
            (Dir::Backward, None) => {
                access
                    .scan_reverse(&prefix, &end, Some(history.limit))
                    .await?
            }
        };
        let mut txids = Vec::with_capacity(rows.len());
        for (_, value) in rows {
            txids.push(Txid::from_slice(&value).ok_or_else(DbError::integrity)?);
        }
        let records = futures::future::try_join_all(
            txids.iter().map(|txid| Self::record(access, root, txid)),
        )
        .await?;
        Ok(records)
    }

    pub fn store(
        edit: &Edit,
        root: &Root,
        txid: &Txid,
        body: &[u8],
        result: &[u8],
    ) -> Result<(), DbError> {
        Self::store_part(edit, Part::Body, root, txid, body)?;
        Self::store_part(edit, Part::Response, root, txid, result)?;
        edit.set_versionstamped_key(&Keys::order_versionstamp_next(root), txid.as_bytes());
        edit.set_versionstamped_value(&Keys::order_txid(root, txid));
        Ok(())
    }

    fn store_part(
        edit: &Edit,
        part: Part,
        root: &Root,
        txid: &Txid,
        bytes: &[u8],
    ) -> Result<(), DbError> {
        let chunks = Chunks::split(bytes);
        for (seq, chunk) in chunks.iter().enumerate() {
            edit.set(&part.chunk_key(root, txid, seq as u64), chunk);
        }
        edit.set_bcs(&part.len_key(root, txid), &(chunks.len() as u64))
    }

    async fn part<R: Reader>(
        access: &R,
        root: &Root,
        txid: &Txid,
        part: Part,
    ) -> Result<Vec<u8>, DbError> {
        let count = Self::count(access, &part.len_key(root, txid)).await?;
        if count > Chunks::limit_for(part.limit()) {
            return Err(DbError::integrity());
        }
        if count == 0 {
            return Ok(Vec::new());
        }
        let prefix = part.prefix(root, txid);
        let end = Prefix::successor(&prefix).ok_or_else(DbError::integrity)?;
        let rows = access.scan(&prefix, &end, Some(count)).await?;
        if rows.len() as u64 != count {
            return Err(DbError::integrity());
        }
        let mut bytes = Vec::with_capacity(count as usize * Chunks::SIZE);
        for (_, value) in rows {
            bytes.extend_from_slice(&value);
        }
        if bytes.len() > part.limit() {
            return Err(DbError::reject(Reject::ResourceLimit));
        }
        Ok(bytes)
    }

    async fn count<R: Reader>(access: &R, key: &[u8]) -> Result<u64, DbError> {
        let bytes = access.get(key).await?.ok_or_else(DbError::integrity)?;
        from_bytes(&bytes).map_err(|_| DbError::integrity())
    }

    async fn versionstamp<R: Reader>(
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<[u8; 10], DbError> {
        let bytes = access
            .get(&Keys::order_txid(root, txid))
            .await?
            .ok_or_else(|| DbError::reject(Reject::NotFound))?;
        bytes.try_into().map_err(|_| DbError::integrity())
    }
}

#[derive(Clone, Copy)]
enum Part {
    Body,
    Response,
}

impl Part {
    fn len_key(self, root: &Root, txid: &Txid) -> Vec<u8> {
        match self {
            Part::Body => Keys::body_len(root, txid),
            Part::Response => Keys::response_len(root, txid),
        }
    }

    fn prefix(self, root: &Root, txid: &Txid) -> Vec<u8> {
        match self {
            Part::Body => Keys::body_prefix(root, txid),
            Part::Response => Keys::response_prefix(root, txid),
        }
    }

    fn chunk_key(self, root: &Root, txid: &Txid, seq: u64) -> Vec<u8> {
        match self {
            Part::Body => Keys::body(root, txid, seq),
            Part::Response => Keys::response(root, txid, seq),
        }
    }

    fn limit(self) -> usize {
        match self {
            Part::Body => Chunks::MAX_BODY,
            Part::Response => Chunks::MAX_RESULT,
        }
    }
}
