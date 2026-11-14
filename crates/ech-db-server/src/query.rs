use ech_db_protocol::error::Reject;
use ech_db_protocol::id::Root;
use ech_db_protocol::value::{Chunks, Outcome, Program, Request};
use tracing::Instrument;

use crate::error::PlatformError;
use crate::fdb::View;
use crate::server::Server;
use crate::timing::{self, Timer};

impl Server {
    #[tracing::instrument(target = "latency", level = "debug", name = "db.query", skip_all,
        fields(root = %root, requests = requests.len()))]
    pub async fn query(
        &self,
        root: Root,
        requests: Vec<Request>,
    ) -> Result<Vec<Outcome>, PlatformError> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        let mut view = self.db.view()?;
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self
                .apply_query(&view, &root, &requests)
                .instrument(tracing::debug_span!(target: "latency", "query.attempt",
                    attempt, requests = requests.len()))
                .await
            {
                Ok(outcomes) => return Ok(outcomes),
                Err(PlatformError::Reject(reject)) => return Err(PlatformError::Reject(reject)),
                Err(PlatformError::Fdb(error)) => {
                    view = timing::measure_async("db_retry", view.retry(error)).await?;
                }
            }
        }
    }

    async fn apply_query(
        &self,
        view: &View,
        root: &Root,
        requests: &[Request],
    ) -> Result<Vec<Outcome>, PlatformError> {
        let _attempt = Timer::new("db_attempt");
        let verify = Timer::new("db_verify");
        tracing::debug_span!(target: "latency", "query.verify", requests = requests.len())
            .in_scope(|| {
                requests
                    .iter()
                    .try_for_each(|request| request.verify(root))
                    .map_err(PlatformError::Reject)
            })?;
        drop(verify);
        if requests
            .iter()
            .any(|request| matches!(request.tx.user.call.program, Program::Txid(_)))
        {
            timing::measure_async("db_grv", view.read_version()).await?;
        }
        let mut outcomes = Vec::with_capacity(requests.len());
        for request in requests {
            let txid = request
                .tx
                .txid(root)
                .map_err(|_| PlatformError::reject(Reject::BadRequest))?;
            let result = self.execute(view, root, &txid, request).await?;
            if result.len() > Chunks::MAX_RESULT {
                return Err(PlatformError::reject(Reject::ResourceLimit));
            }
            outcomes.push(Outcome { txid, result });
        }
        Ok(outcomes)
    }
}
