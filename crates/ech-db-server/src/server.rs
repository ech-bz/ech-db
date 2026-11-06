use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use wasmi::Module;

use ech_db_protocol::id::{Root, Txid};
use ech_db_protocol::value::{Program, Request};

use crate::error::DbError;
use crate::exec::{Executor, ReadMetrics};
use crate::fdb::{Db, Reader};
use crate::programs::Programs;
use crate::wasm::Profile;

pub struct Server {
    pub db: Db,
    pub executor: Executor,
    pub programs: Programs,
}

pub(crate) enum PreparedProgram {
    Native,
    Wasm {
        id: Txid,
        module: Module,
        load_ms: f64,
    },
}

impl Server {
    pub fn new(db: Db) -> Self {
        let engine = Profile::engine();
        Self {
            programs: Programs::new(engine.clone()),
            executor: Executor::new(engine),
            db,
        }
    }

    pub async fn run(self: Arc<Self>, listen: SocketAddr) -> Result<(), std::io::Error> {
        let listener = tokio::net::TcpListener::bind(listen).await?;
        axum::serve(listener, self.router()).await
    }

    pub async fn execute<R: Reader>(
        &self,
        access: &R,
        root: &Root,
        txid: &Txid,
        request: &Request,
    ) -> Result<Vec<u8>, DbError> {
        let prepared = self.prepare_program(access, root, request).await?;
        self.execute_prepared(access, root, txid, request, prepared)
            .await
    }

    pub(crate) async fn prepare_program<R: Reader>(
        &self,
        access: &R,
        root: &Root,
        request: &Request,
    ) -> Result<PreparedProgram, DbError> {
        let started = Instant::now();
        match request.tx.user.call.program {
            Program::Txid(id) => {
                let module = self.programs.module(access, root, &id).await?;
                Ok(PreparedProgram::Wasm {
                    id,
                    module,
                    load_ms: started.elapsed().as_secs_f64() * 1000.0,
                })
            }
            Program::Native => Ok(PreparedProgram::Native),
        }
    }

    pub(crate) async fn execute_prepared<R: Reader>(
        &self,
        access: &R,
        root: &Root,
        txid: &Txid,
        request: &Request,
        prepared: PreparedProgram,
    ) -> Result<Vec<u8>, DbError> {
        match prepared {
            PreparedProgram::Wasm {
                id,
                module,
                load_ms: module_ms,
            } => {
                let execution_started = Instant::now();
                let mut reads = ReadMetrics::default();
                let result = self
                    .executor
                    .run(access, &module, root, request, &mut reads)
                    .await;
                let execution_ms = execution_started.elapsed().as_secs_f64() * 1000.0;
                if module_ms + execution_ms >= 100.0 {
                    tracing::info!(
                        %root,
                        %txid,
                        program = %id,
                        function = %request.tx.user.call.function,
                        read_only = access.read_only(),
                        module_ms,
                        execution_ms,
                        get_calls = reads.get_calls,
                        get_keys = reads.get_keys,
                        get_ms = reads.get_time.as_secs_f64() * 1000.0,
                        range_calls = reads.range_calls,
                        range_rows = reads.range_rows,
                        range_ms = reads.range_time.as_secs_f64() * 1000.0,
                        success = result.is_ok(),
                        "slow program execution"
                    );
                }
                result
            }
            PreparedProgram::Native => self.programs.native(txid, request),
        }
    }
}
