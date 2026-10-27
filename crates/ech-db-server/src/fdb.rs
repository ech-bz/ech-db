use std::future::Future;

use foundationdb::options::MutationType;
use foundationdb::{Database, FdbError, RangeOption, Transaction};
use futures::TryStreamExt;
use serde::Serialize;

use ech_db_protocol::error::Reject;

use crate::error::DbError;

pub type Row = (Vec<u8>, Vec<u8>);

pub trait Reader {
    type Writes: Mutator;

    fn writes(&self) -> &Self::Writes;

    fn get(&self, key: &[u8]) -> impl Future<Output = Result<Option<Vec<u8>>, DbError>> + Send;

    fn scan(
        &self,
        begin: &[u8],
        end: &[u8],
        limit: Option<u64>,
    ) -> impl Future<Output = Result<Vec<Row>, DbError>> + Send;

    fn scan_reverse(
        &self,
        begin: &[u8],
        end: &[u8],
        limit: Option<u64>,
    ) -> impl Future<Output = Result<Vec<Row>, DbError>> + Send;
}

pub trait Mutator {
    fn put(&self, key: &[u8], value: &[u8]) -> Result<(), DbError>;

    fn clear(&self, key: &[u8]) -> Result<(), DbError>;
}

pub struct Denied;

impl Mutator for Denied {
    fn put(&self, _key: &[u8], _value: &[u8]) -> Result<(), DbError> {
        Err(DbError::reject(Reject::ReadOnly))
    }

    fn clear(&self, _key: &[u8]) -> Result<(), DbError> {
        Err(DbError::reject(Reject::ReadOnly))
    }
}

pub struct Db {
    database: Database,
}

impl Db {
    pub fn open(cluster_file: &str) -> Result<Self, FdbError> {
        Ok(Self {
            database: Database::new(Some(cluster_file))?,
        })
    }

    pub fn view(&self) -> Result<View, DbError> {
        Ok(View {
            trx: self.database.create_trx()?,
        })
    }

    pub fn edit(&self) -> Result<Edit, DbError> {
        Ok(Edit { view: self.view()? })
    }

    pub async fn pause(&self, error: FdbError) -> Result<(), DbError> {
        let trx = self.database.create_trx()?;
        trx.on_error(error).await.map(|_| ()).map_err(DbError::Fdb)
    }
}

pub struct View {
    trx: Transaction,
}

impl View {
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DbError> {
        self.trx
            .get(key, false)
            .await
            .map(|value| value.map(|slice| slice.to_vec()))
            .map_err(DbError::Fdb)
    }

    pub async fn scan(
        &self,
        begin: &[u8],
        end: &[u8],
        limit: Option<u64>,
    ) -> Result<Vec<Row>, DbError> {
        self.collect(begin, end, limit, false).await
    }

    pub async fn scan_reverse(
        &self,
        begin: &[u8],
        end: &[u8],
        limit: Option<u64>,
    ) -> Result<Vec<Row>, DbError> {
        self.collect(begin, end, limit, true).await
    }

    async fn collect(
        &self,
        begin: &[u8],
        end: &[u8],
        limit: Option<u64>,
        reverse: bool,
    ) -> Result<Vec<Row>, DbError> {
        let mut option = RangeOption::from((begin.to_vec(), end.to_vec()));
        option.reverse = reverse;
        if let Some(limit) = limit {
            option.limit =
                Some(usize::try_from(limit).map_err(|_| DbError::reject(Reject::ResourceLimit))?);
        }
        let mut stream = self.trx.get_ranges_keyvalues(option, false);
        let mut rows = Vec::new();
        while let Some(kv) = stream.try_next().await.map_err(DbError::Fdb)? {
            rows.push((kv.key().to_vec(), kv.value().to_vec()));
            if let Some(limit) = limit {
                if rows.len() as u64 >= limit {
                    break;
                }
            }
        }
        Ok(rows)
    }
}

impl Reader for View {
    type Writes = Denied;

    fn writes(&self) -> &Denied {
        static DENIED: Denied = Denied;
        &DENIED
    }

    fn get(&self, key: &[u8]) -> impl Future<Output = Result<Option<Vec<u8>>, DbError>> + Send {
        View::get(self, key)
    }

    fn scan(
        &self,
        begin: &[u8],
        end: &[u8],
        limit: Option<u64>,
    ) -> impl Future<Output = Result<Vec<Row>, DbError>> + Send {
        View::scan(self, begin, end, limit)
    }

    fn scan_reverse(
        &self,
        begin: &[u8],
        end: &[u8],
        limit: Option<u64>,
    ) -> impl Future<Output = Result<Vec<Row>, DbError>> + Send {
        View::scan_reverse(self, begin, end, limit)
    }
}

pub struct Edit {
    view: View,
}

impl Edit {
    pub fn set(&self, key: &[u8], value: &[u8]) {
        self.view.trx.set(key, value);
    }

    pub fn set_bcs<T: Serialize>(&self, key: &[u8], value: &T) -> Result<(), DbError> {
        let bytes = bcs::to_bytes(value).map_err(|_| DbError::integrity())?;
        self.set(key, &bytes);
        Ok(())
    }

    pub fn set_versionstamped_key(&self, key: &[u8], value: &[u8]) {
        self.view
            .trx
            .atomic_op(key, value, MutationType::SetVersionstampedKey);
    }

    pub fn set_versionstamped_value(&self, key: &[u8]) {
        self.view
            .trx
            .atomic_op(key, &[0u8; 14], MutationType::SetVersionstampedValue);
    }

    pub async fn commit(self) -> Result<(), FdbError> {
        self.view
            .trx
            .commit()
            .await
            .map(|_| ())
            .map_err(FdbError::from)
    }
}

impl Mutator for Edit {
    fn put(&self, key: &[u8], value: &[u8]) -> Result<(), DbError> {
        self.set(key, value);
        Ok(())
    }

    fn clear(&self, key: &[u8]) -> Result<(), DbError> {
        self.view.trx.clear(key);
        Ok(())
    }
}

impl Reader for Edit {
    type Writes = Edit;

    fn writes(&self) -> &Edit {
        self
    }

    fn get(&self, key: &[u8]) -> impl Future<Output = Result<Option<Vec<u8>>, DbError>> + Send {
        View::get(&self.view, key)
    }

    fn scan(
        &self,
        begin: &[u8],
        end: &[u8],
        limit: Option<u64>,
    ) -> impl Future<Output = Result<Vec<Row>, DbError>> + Send {
        View::scan(&self.view, begin, end, limit)
    }

    fn scan_reverse(
        &self,
        begin: &[u8],
        end: &[u8],
        limit: Option<u64>,
    ) -> impl Future<Output = Result<Vec<Row>, DbError>> + Send {
        View::scan_reverse(&self.view, begin, end, limit)
    }
}
