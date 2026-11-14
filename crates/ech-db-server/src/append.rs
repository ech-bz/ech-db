use std::collections::HashSet;
use std::time::Instant;

use bcs::to_bytes;

use ech_db_protocol::error::Reject;
use ech_db_protocol::id::Root;
use ech_db_protocol::value::{Chunks, Outcome, Request};

use crate::error::PlatformError;
use crate::fdb::Edit;
use crate::log::Log;
use crate::server::Server;

impl Server {
    #[tracing::instrument(target = "latency", level = "debug", name = "db.append", skip_all,
        fields(root = %root, requests = requests.len()))]
    pub async fn append(
        &self,
        root: Root,
        requests: Vec<Request>,
    ) -> Result<Vec<Outcome>, PlatformError> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        if requests.len() > u16::MAX as usize {
            return Err(PlatformError::reject(Reject::ResourceLimit));
        }
        let started = Instant::now();
        let mut edit = self.db.edit()?;
        let mut attempts = 0u64;
        let outcomes = loop {
            attempts += 1;
            let outcomes = match self.apply_append(&edit, &root, &requests).await {
                Ok(outcomes) => outcomes,
                Err(PlatformError::Reject(reject)) => return Err(PlatformError::Reject(reject)),
                Err(PlatformError::Fdb(error)) => {
                    tracing::info!(attempts, %error, phase = "apply", "retrying append");
                    edit = edit.retry(error).await?;
                    continue;
                }
            };
            let commit_started = Instant::now();
            let committed = edit.commit().await;
            let commit_ms = commit_started.elapsed().as_secs_f64() * 1000.0;
            if commit_ms >= 50.0 || committed.is_err() {
                tracing::info!(
                    attempts,
                    requests = requests.len(),
                    commit_ms,
                    success = committed.is_ok(),
                    "append commit"
                );
            }
            match committed {
                Ok(()) => break outcomes,
                Err(error) if error.is_maybe_committed() => {
                    tracing::info!(attempts, %error, phase = "commit_unknown",
                        "checking append outcome");
                    let mut stored = true;
                    for outcome in &outcomes {
                        if !Log::exists(&self.db.view()?, &root, &outcome.txid).await? {
                            stored = false;
                            break;
                        }
                    }
                    if stored {
                        break outcomes;
                    }
                    edit = error.retry().await?;
                }
                Err(error) => {
                    tracing::info!(attempts, %error, phase = "commit", "retrying append");
                    edit = error.retry().await?;
                }
            }
        };
        let total_ms = started.elapsed().as_secs_f64() * 1000.0;
        if total_ms >= 250.0 || attempts > 1 {
            tracing::info!(attempts, requests = requests.len(), total_ms, "slow append");
        }
        Ok(outcomes)
    }

    async fn apply_append(
        &self,
        edit: &Edit,
        root: &Root,
        requests: &[Request],
    ) -> Result<Vec<Outcome>, PlatformError> {
        let started = Instant::now();
        let mut outcomes = Vec::with_capacity(requests.len());
        let mut seen = HashSet::with_capacity(requests.len());
        let mut verify_ms = 0.0;
        let mut lookup_ms = 0.0;
        let mut execution_ms = 0.0;
        let mut stage_ms = 0.0;
        for (index, request) in requests.iter().enumerate() {
            let verify_started = Instant::now();
            request.verify(root).map_err(PlatformError::Reject)?;
            verify_ms += verify_started.elapsed().as_secs_f64() * 1000.0;
            let txid = request
                .tx
                .txid(root)
                .map_err(|_| PlatformError::reject(Reject::BadRequest))?;
            if !seen.insert(*txid.as_array()) {
                return Err(PlatformError::reject(Reject::Duplicate));
            }
            let lookup_started = Instant::now();
            let (exists, prepared) = futures::join!(
                Log::exists(edit, root, &txid),
                self.prepare_program(edit, root, request),
            );
            if exists? {
                return Err(PlatformError::reject(Reject::Duplicate));
            }
            let prepared = prepared?;
            lookup_ms += lookup_started.elapsed().as_secs_f64() * 1000.0;
            let body =
                to_bytes(&request.tx).map_err(|_| PlatformError::reject(Reject::BadRequest))?;
            if body.len() > Chunks::MAX_BODY {
                return Err(PlatformError::reject(Reject::ResourceLimit));
            }
            let execution_started = Instant::now();
            let result = self
                .execute_prepared(edit, root, &txid, request, prepared)
                .await?;
            execution_ms += execution_started.elapsed().as_secs_f64() * 1000.0;
            if result.len() > Chunks::MAX_RESULT {
                return Err(PlatformError::reject(Reject::ResourceLimit));
            }
            let stage_started = Instant::now();
            let position =
                u16::try_from(index).map_err(|_| PlatformError::reject(Reject::ResourceLimit))?;
            Log::store(
                edit,
                root,
                &txid,
                &request.tx.user.author,
                &body,
                &result,
                position,
            )?;
            stage_ms += stage_started.elapsed().as_secs_f64() * 1000.0;
            outcomes.push(Outcome { txid, result });
        }
        if started.elapsed().as_millis() >= 100 {
            tracing::info!(
                verify_ms,
                lookup_ms,
                execution_ms,
                stage_ms,
                requests = requests.len(),
                "slow append preparation"
            );
        }
        Ok(outcomes)
    }
}
