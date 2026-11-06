use ech_db_protocol::error::Reject;
use ech_db_protocol::id::{Root, Txid};
use ech_db_protocol::value::{Chunks, Outcome, Program, Request};
use tracing::Instrument;

use crate::error::DbError;
use crate::fdb::View;
use crate::server::Server;
use crate::timing::{self, Timer};

impl Server {
    pub async fn query(&self, root: Root, request: Request) -> Result<Outcome, DbError> {
        let txid = request
            .tx
            .txid(&root)
            .map_err(|_| DbError::reject(Reject::BadRequest))?;
        tracing::Span::current().record("txid", tracing::field::display(txid));
        let mut view = self.db.view()?;
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self
                .apply_query(&view, &root, txid, &request)
                .instrument(tracing::debug_span!(target: "latency", "query.attempt", attempt))
                .await
            {
                Ok(outcome) => return Ok(outcome),
                Err(DbError::Reject(reject)) => return Err(DbError::Reject(reject)),
                Err(DbError::Fdb(error)) => {
                    view = timing::measure_async("db_retry", view.retry(error)).await?;
                }
            }
        }
    }

    async fn apply_query(
        &self,
        view: &View,
        root: &Root,
        txid: Txid,
        request: &Request,
    ) -> Result<Outcome, DbError> {
        let _attempt = Timer::new("db_attempt");
        let verify = Timer::new("db_verify");
        tracing::debug_span!(target: "latency", "query.verify")
            .in_scope(|| request.verify(root))
            .map_err(DbError::Reject)?;
        drop(verify);
        if matches!(request.tx.user.call.program, Program::Txid(_)) {
            timing::measure_async("db_grv", view.read_version()).await?;
        }
        let result = self.execute(view, root, &txid, request).await?;
        if result.len() > Chunks::MAX_RESULT {
            return Err(DbError::reject(Reject::ResourceLimit));
        }
        Ok(Outcome { txid, result })
    }
}
