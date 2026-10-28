use ech_db_protocol::error::Reject;
use ech_db_protocol::id::Root;
use ech_db_protocol::value::{Chunks, Outcome, Request};

use crate::error::DbError;
use crate::fdb::View;
use crate::server::Server;

impl Server {
    pub async fn query(&self, root: Root, request: Request) -> Result<Outcome, DbError> {
        loop {
            let view = self.db.view()?;
            match self.apply_query(&view, &root, &request).await {
                Ok(outcome) => return Ok(outcome),
                Err(DbError::Reject(reject)) => return Err(DbError::Reject(reject)),
                Err(DbError::Fdb(error)) => self.db.pause(error).await?,
            }
        }
    }

    async fn apply_query(
        &self,
        view: &View,
        root: &Root,
        request: &Request,
    ) -> Result<Outcome, DbError> {
        if !request.tx.verify(root) {
            return Err(DbError::reject(Reject::BadSignature));
        }
        let txid = request
            .tx
            .call
            .txid(root)
            .map_err(|_| DbError::reject(Reject::BadRequest))?;
        let result = self.execute(view, root, &txid, request).await?;
        if result.len() > Chunks::MAX_RESULT {
            return Err(DbError::reject(Reject::ResourceLimit));
        }
        Ok(Outcome { txid, result })
    }
}
