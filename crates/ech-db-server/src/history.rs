use ech_db_protocol::id::Root;
use ech_db_protocol::value::Record;

use crate::error::PlatformError;
use crate::log::{History, Log};
use crate::server::Server;

impl Server {
    pub async fn history(
        &self,
        root: Root,
        history: History,
    ) -> Result<Vec<Record>, PlatformError> {
        let mut view = self.db.view()?;
        loop {
            match Log::page(&view, &root, &history).await {
                Ok(records) => return Ok(records),
                Err(PlatformError::Reject(reject)) => return Err(PlatformError::Reject(reject)),
                Err(PlatformError::Fdb(error)) => view = view.retry(error).await?,
            }
        }
    }
}
