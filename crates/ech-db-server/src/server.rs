use std::net::SocketAddr;
use std::sync::Arc;

use axum::serve::ListenerExt;
use tracing::Instrument;
use wasmtime::Module;

use ech_db_protocol::id::{Root, Txid};
use ech_db_protocol::value::{Program, Request};

use crate::error::DbError;
use crate::exec::Executor;
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
    Wasm { id: Txid, module: Module },
}

impl Server {
    pub fn new(db: Db) -> Result<Self, wasmtime::Error> {
        let engine = Profile::engine()?;
        Ok(Self {
            programs: Programs::new(engine.clone()),
            executor: Executor::new(engine)?,
            db,
        })
    }

    pub async fn run(self: Arc<Self>, listen: SocketAddr) -> Result<(), std::io::Error> {
        let listener = tokio::net::TcpListener::bind(listen)
            .await?
            .tap_io(|stream| {
                if let Err(error) = stream.set_nodelay(true) {
                    tracing::warn!(%error, "failed to enable TCP_NODELAY");
                }
            });
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
        match request.tx.user.call.program {
            Program::Txid(id) => {
                let module = self.programs.module(access, root, &id).await?;
                Ok(PreparedProgram::Wasm { id, module })
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
            PreparedProgram::Wasm { id, module } => {
                self.executor
                    .run(access, &module, root, request)
                    .instrument(tracing::debug_span!(target: "latency", "wasm.execute",
                    %root, %txid, program = %id, function = %request.tx.user.call.function,
                    read_only = access.read_only()))
                    .await
            }
            PreparedProgram::Native => self.programs.native(txid, request).await,
        }
    }
}
