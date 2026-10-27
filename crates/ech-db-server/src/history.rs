use ech_db_protocol::id::Root;
use ech_db_protocol::value::Record;

use crate::error::DbError;
use crate::log::{History, Log};
use crate::server::Server;

impl Server {
    pub async fn history(&self, root: Root, history: History) -> Result<Vec<Record>, DbError> {
        loop {
            let view = self.db.view()?;
            match Log::page(&view, &root, &history).await {
                Ok(records) => return Ok(records),
                Err(DbError::Reject(reject)) => return Err(DbError::Reject(reject)),
                Err(DbError::Fdb(error)) => self.db.pause(error).await?,
            }
        }
    }
}
