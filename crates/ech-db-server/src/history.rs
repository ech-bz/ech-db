use ech_db_protocol::id::Root;
use ech_db_protocol::value::Record;

use crate::error::DbError;
use crate::log::{History, Log};
use crate::server::Server;

impl Server {
    pub async fn history(&self, root: Root, history: History) -> Result<Vec<Record>, DbError> {
        let mut view = self.db.view()?;
        loop {
            match Log::page(&view, &root, &history).await {
                Ok(records) => return Ok(records),
                Err(DbError::Reject(reject)) => return Err(DbError::Reject(reject)),
                Err(DbError::Fdb(error)) => view = view.retry(error).await?,
            }
        }
    }
}
