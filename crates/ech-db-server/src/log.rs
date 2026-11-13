use bcs::from_bytes;

use ech_db_protocol::error::Reject;
use ech_db_protocol::id::{Root, Txid};
use ech_db_protocol::keys::{Keys, Prefix};
use ech_db_protocol::value::{Author, Chunks, Esid, Record, Tx};

use crate::error::PlatformError;
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
    pub esid: Option<Esid>,
}

pub struct Log;

impl Log {
    pub async fn exists<R: Reader>(
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<bool, PlatformError> {
        Ok(access.get(&Keys::order_txid(root, txid)).await?.is_some())
    }

    pub async fn tx<R: Reader>(access: &R, root: &Root, txid: &Txid) -> Result<Tx, PlatformError> {
        let body = Self::body(access, root, txid).await?;
        let tx: Tx = from_bytes(&body).map_err(|_| PlatformError::integrity())?;
        let computed = tx.txid(root).map_err(|_| PlatformError::integrity())?;
        if computed != *txid {
            return Err(PlatformError::integrity());
        }
        Ok(tx)
    }

    pub async fn body<R: Reader>(
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<Vec<u8>, PlatformError> {
        Self::part(access, root, txid, Part::Body).await
    }

    pub async fn response<R: Reader>(
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<Vec<u8>, PlatformError> {
        Self::part(access, root, txid, Part::Response).await
    }

    pub async fn record<R: Reader>(
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<Record, PlatformError> {
        let (tx, result) = futures::try_join!(
            Self::tx(access, root, txid),
            Self::response(access, root, txid),
        )?;
        Ok(Record { tx, result })
    }

    pub async fn page<R: Reader>(
        access: &R,
        root: &Root,
        history: &History,
    ) -> Result<Vec<Record>, PlatformError> {
        let prefix = match &history.esid {
            Some(esid) => Keys::author_entries_prefix(root, esid),
            None => Keys::order_versionstamp_prefix(root),
        };
        let end = Prefix::successor(&prefix).ok_or_else(PlatformError::integrity)?;
        let from = match history.from {
            Some(txid) => {
                let versionstamp = Self::versionstamp(access, root, &txid).await?;
                Some(match &history.esid {
                    Some(esid) => Keys::author_versionstamp(root, esid, versionstamp),
                    None => Keys::order_versionstamp(root, versionstamp),
                })
            }
            None => None,
        };
        let rows = match (history.dir, from) {
            (Dir::Forward, Some(begin)) => access.scan(&begin, &end, Some(history.limit)).await?,
            (Dir::Forward, None) => access.scan(&prefix, &end, Some(history.limit)).await?,
            (Dir::Backward, Some(mut after)) => {
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
            txids.push(Txid::from_slice(&value).ok_or_else(PlatformError::integrity)?);
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
        author: &Author,
        body: &[u8],
        result: &[u8],
    ) -> Result<(), PlatformError> {
        Self::store_part(edit, Part::Body, root, txid, body)?;
        Self::store_part(edit, Part::Response, root, txid, result)?;
        edit.set_versionstamped_key(&Keys::order_versionstamp_next(root), txid.as_bytes());
        edit.set_versionstamped_value(&Keys::order_txid(root, txid));
        if let Author::User { esid, .. } = author {
            edit.add(&Keys::author_length(root, esid), 1);
            edit.set_versionstamped_key(
                &Keys::author_versionstamp_next(root, esid),
                txid.as_bytes(),
            );
        }
        Ok(())
    }

    fn store_part(
        edit: &Edit,
        part: Part,
        root: &Root,
        txid: &Txid,
        bytes: &[u8],
    ) -> Result<(), PlatformError> {
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
    ) -> Result<Vec<u8>, PlatformError> {
        let count = Self::count(access, &part.len_key(root, txid)).await?;
        if count > Chunks::limit_for(part.limit()) {
            return Err(PlatformError::integrity());
        }
        if count == 0 {
            return Ok(Vec::new());
        }
        let prefix = part.prefix(root, txid);
        let end = Prefix::successor(&prefix).ok_or_else(PlatformError::integrity)?;
        let rows = access.scan(&prefix, &end, Some(count)).await?;
        if rows.len() as u64 != count {
            return Err(PlatformError::integrity());
        }
        let mut bytes = Vec::with_capacity(count as usize * Chunks::SIZE);
        for (_, value) in rows {
            bytes.extend_from_slice(&value);
        }
        if bytes.len() > part.limit() {
            return Err(PlatformError::reject(Reject::ResourceLimit));
        }
        Ok(bytes)
    }

    async fn count<R: Reader>(access: &R, key: &[u8]) -> Result<u64, PlatformError> {
        let bytes = access
            .get(key)
            .await?
            .ok_or_else(PlatformError::integrity)?;
        from_bytes(&bytes).map_err(|_| PlatformError::integrity())
    }

    async fn versionstamp<R: Reader>(
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<[u8; 10], PlatformError> {
        let bytes = access
            .get(&Keys::order_txid(root, txid))
            .await?
            .ok_or_else(|| PlatformError::reject(Reject::NotFound))?;
        bytes.try_into().map_err(|_| PlatformError::integrity())
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
