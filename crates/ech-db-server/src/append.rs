use bcs::to_bytes;

use ech_db_protocol::error::Reject;
use ech_db_protocol::id::Root;
use ech_db_protocol::value::{Chunks, Outcome, Request};

use crate::error::DbError;
use crate::fdb::Edit;
use crate::log::Log;
use crate::server::Server;

impl Server {
    pub async fn append(&self, root: Root, request: Request) -> Result<Outcome, DbError> {
        loop {
            let edit = self.db.edit()?;
            let outcome = match self.apply_append(&edit, &root, &request).await {
                Ok(outcome) => outcome,
                Err(DbError::Reject(reject)) => return Err(DbError::Reject(reject)),
                Err(DbError::Fdb(error)) => {
                    self.db.pause(error).await?;
                    continue;
                }
            };
            match edit.commit().await {
                Ok(()) => return Ok(outcome),
                Err(error) if error.is_maybe_committed() => {
                    if Log::exists(&self.db.view()?, &root, &outcome.txid).await? {
                        return Ok(outcome);
                    }
                    self.db.pause(error).await?;
                }
                Err(error) => self.db.pause(error).await?,
            }
        }
    }

    async fn apply_append(
        &self,
        edit: &Edit,
        root: &Root,
        request: &Request,
    ) -> Result<Outcome, DbError> {
        if !request.tx.verify(root) {
            return Err(DbError::reject(Reject::BadSignature));
        }
        if !request.tx.call.ctx.verify(&request.private) {
            return Err(DbError::reject(Reject::Commitment));
        }
        let txid = request
            .tx
            .call
            .txid(root)
            .map_err(|_| DbError::reject(Reject::BadRequest))?;
        if Log::exists(edit, root, &txid).await? {
            return Err(DbError::reject(Reject::Duplicate));
        }
        let body = to_bytes(&request.tx).map_err(|_| DbError::reject(Reject::BadRequest))?;
        if body.len() > Chunks::MAX_BODY {
            return Err(DbError::reject(Reject::ResourceLimit));
        }
        let result = self.execute(edit, root, &txid, request).await?;
        if result.len() > Chunks::MAX_RESULT {
            return Err(DbError::reject(Reject::ResourceLimit));
        }
        Log::store(edit, root, &txid, &body, &result)?;
        Ok(Outcome { txid, result })
    }
}
