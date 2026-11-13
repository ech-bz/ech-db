use ech_db_protocol::id::Root;
use ech_db_protocol::keys::Keys;
use ech_db_protocol::value::Record;

use crate::error::PlatformError;
use crate::log::{History, Log};
use crate::server::Server;

impl Server {
    pub async fn history(
        &self,
        root: Root,
        history: History,
    ) -> Result<(Vec<Record>, Option<u64>), PlatformError> {
        let mut view = self.db.view()?;
        loop {
            let length = async {
                let Some(esid) = history.esid.filter(|_| history.from.is_none()) else {
                    return Ok(None);
                };
                let bytes = view.get(&Keys::author_length(&root, &esid)).await?;
                let length = match bytes {
                    Some(bytes) => {
                        bcs::from_bytes::<u64>(&bytes).map_err(|_| PlatformError::integrity())?
                    }
                    None => 0,
                };
                Ok(Some(length))
            };
            match futures::try_join!(Log::page(&view, &root, &history), length) {
                Ok((records, length)) => {
                    if length.is_some_and(|length| length < records.len() as u64) {
                        return Err(PlatformError::integrity());
                    }
                    return Ok((records, length));
                }
                Err(PlatformError::Reject(reject)) => return Err(PlatformError::Reject(reject)),
                Err(PlatformError::Fdb(error)) => view = view.retry(error).await?,
            }
        }
    }
}
