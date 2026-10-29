use std::net::SocketAddr;
use std::sync::Arc;

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
        match request.tx.submission.call.program {
            Program::Txid(program) => {
                let module = self.programs.module(access, root, &program).await?;
                self.executor.run(access, &module, root, request).await
            }
            Program::Native => self.programs.native(txid, request),
        }
    }
}
