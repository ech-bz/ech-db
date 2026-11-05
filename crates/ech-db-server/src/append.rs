use std::time::Instant;

use bcs::to_bytes;

use ech_db_protocol::error::Reject;
use ech_db_protocol::id::Root;
use ech_db_protocol::value::{Chunks, Outcome, Request};

use crate::error::DbError;
use crate::fdb::Edit;
use crate::log::Log;
use crate::server::Server;

impl Server {
    #[tracing::instrument(skip_all, fields(%root, function = %request.tx.user.call.function, txid = tracing::field::Empty))]
    pub async fn append(&self, root: Root, request: Request) -> Result<Outcome, DbError> {
        let started = Instant::now();
        let mut attempts = 0u64;
        let outcome = loop {
            attempts += 1;
            let edit = self.db.edit()?;
            let outcome = match self.apply_append(&edit, &root, &request).await {
                Ok(outcome) => outcome,
                Err(DbError::Reject(reject)) => return Err(DbError::Reject(reject)),
                Err(DbError::Fdb(error)) => {
                    tracing::info!(attempts, %error, phase = "apply", "retrying append");
                    self.db.pause(error).await?;
                    continue;
                }
            };
            let commit_started = Instant::now();
            let committed = edit.commit().await;
            let commit_ms = commit_started.elapsed().as_secs_f64() * 1000.0;
            if commit_ms >= 50.0 || committed.is_err() {
                tracing::info!(
                    attempts,
                    commit_ms,
                    success = committed.is_ok(),
                    "append commit"
                );
            }
            match committed {
                Ok(()) => break outcome,
                Err(error) if error.is_maybe_committed() => {
                    tracing::info!(attempts, %error, phase = "commit_unknown", "checking append outcome");
                    if Log::exists(&self.db.view()?, &root, &outcome.txid).await? {
                        break outcome;
                    }
                    self.db.pause(error).await?;
                }
                Err(error) => {
                    tracing::info!(attempts, %error, phase = "commit", "retrying append");
                    self.db.pause(error).await?;
                }
            }
        };
        let total_ms = started.elapsed().as_secs_f64() * 1000.0;
        if total_ms >= 250.0 || attempts > 1 {
            tracing::info!(attempts, total_ms, "slow append");
        }
        Ok(outcome)
    }

    async fn apply_append(
        &self,
        edit: &Edit,
        root: &Root,
        request: &Request,
    ) -> Result<Outcome, DbError> {
        let started = Instant::now();
        request.verify(root).map_err(DbError::Reject)?;
        let txid = request
            .tx
            .txid(root)
            .map_err(|_| DbError::reject(Reject::BadRequest))?;
        tracing::Span::current().record("txid", tracing::field::display(txid));
        let verify_ms = started.elapsed().as_secs_f64() * 1000.0;
        let lookup_started = Instant::now();
        let (exists, prepared) = futures::join!(
            Log::exists(edit, root, &txid),
            self.prepare_program(edit, root, request),
        );
        if exists? {
            return Err(DbError::reject(Reject::Duplicate));
        }
        let prepared = prepared?;
        let lookup_ms = lookup_started.elapsed().as_secs_f64() * 1000.0;
        let body = to_bytes(&request.tx).map_err(|_| DbError::reject(Reject::BadRequest))?;
        if body.len() > Chunks::MAX_BODY {
            return Err(DbError::reject(Reject::ResourceLimit));
        }
        let execution_started = Instant::now();
        let result = self
            .execute_prepared(edit, root, &txid, request, prepared)
            .await?;
        let execution_ms = execution_started.elapsed().as_secs_f64() * 1000.0;
        if result.len() > Chunks::MAX_RESULT {
            return Err(DbError::reject(Reject::ResourceLimit));
        }
        let stage_started = Instant::now();
        Log::store(edit, root, &txid, &body, &result)?;
        let stage_ms = stage_started.elapsed().as_secs_f64() * 1000.0;
        if started.elapsed().as_millis() >= 100 {
            tracing::info!(
                verify_ms,
                lookup_ms,
                execution_ms,
                stage_ms,
                "slow append preparation"
            );
        }
        Ok(Outcome { txid, result })
    }
}
