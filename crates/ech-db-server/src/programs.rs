use std::collections::HashMap;
use std::sync::Mutex;

use tracing::Instrument;
use wasmtime::{Engine, Module};

use ech_db_protocol::error::Reject;
use ech_db_protocol::id::{Root, Txid};
use ech_db_protocol::value::{Program, Request};

use crate::abi::Abi;
use crate::error::DbError;
use crate::fdb::Reader;
use crate::log::Log;
use crate::timing::Timer;

pub struct Programs {
    engine: Engine,
    cache: Mutex<HashMap<Txid, Module>>,
    compile_lock: tokio::sync::Mutex<()>,
}

impl Programs {
    pub const UPLOAD_PROGRAM: &'static str = "upload_program";

    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            cache: Mutex::new(HashMap::new()),
            compile_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn native(&self, txid: &Txid, request: &Request) -> Result<Vec<u8>, DbError> {
        match request.tx.user.call.function.as_str() {
            Self::UPLOAD_PROGRAM => self.upload(txid, &request.tx.user.call.args).await,
            function => Err(DbError::reject(Reject::UnknownNative {
                function: function.to_string(),
            })),
        }
    }

    #[tracing::instrument(target = "latency", level = "debug", name = "wasm.module", skip_all,
        fields(program = %txid, cached = tracing::field::Empty))]
    pub async fn module<R: Reader>(
        &self,
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<Module, DbError> {
        let _module = Timer::new("db_module");
        let check = Timer::new("db_module_check");
        if !Log::exists(access, root, txid)
            .instrument(tracing::debug_span!(target: "latency", "wasm.check_program"))
            .await?
        {
            return Err(DbError::reject(Reject::ProgramNotFound));
        }
        drop(check);
        if let Some(module) = self.get(txid) {
            tracing::Span::current().record("cached", true);
            return Ok(module);
        }
        let _compilation = self.compile_lock.lock().await;
        if let Some(module) = self.get(txid) {
            tracing::Span::current().record("cached", true);
            return Ok(module);
        }
        tracing::Span::current().record("cached", false);
        let load = Timer::new("db_module_load");
        let wasm = self
            .wasm(access, root, txid)
            .instrument(tracing::debug_span!(target: "latency", "wasm.load_program"))
            .await?;
        drop(load);
        let _compile = Timer::new("db_module_compile");
        let module = self.compile(wasm).await?;
        self.cache(*txid, module.clone());
        Ok(module)
    }

    async fn upload(&self, txid: &Txid, wasm: &[u8]) -> Result<Vec<u8>, DbError> {
        let _compilation = self.compile_lock.lock().await;
        if self.get(txid).is_none() {
            let module = self.compile(wasm.to_vec()).await?;
            self.cache(*txid, module);
        }
        Ok(Vec::new())
    }

    async fn compile(&self, wasm: Vec<u8>) -> Result<Module, DbError> {
        let engine = self.engine.clone();
        let span = tracing::debug_span!(target: "latency", "wasm.compile", bytes = wasm.len());
        tokio::task::spawn_blocking(move || {
            span.in_scope(|| {
                Abi::check_bytes(&wasm)?;
                let module = Module::new(&engine, &wasm)
                    .map_err(|_| DbError::reject(Reject::InvalidWasm))?;
                Abi::check(&module)?;
                Ok(module)
            })
        })
        .await
        .map_err(|_| DbError::integrity())?
    }

    async fn wasm<R: Reader>(
        &self,
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<Vec<u8>, DbError> {
        let tx = Log::tx(access, root, txid).await?;
        if !matches!(tx.user.call.program, Program::Native)
            || tx.user.call.function != Self::UPLOAD_PROGRAM
        {
            return Err(DbError::reject(Reject::NotAProgram));
        }
        Ok(tx.user.call.args)
    }

    fn get(&self, txid: &Txid) -> Option<Module> {
        let cache = self
            .cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        cache.get(txid).cloned()
    }

    fn cache(&self, txid: Txid, module: Module) {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        cache.insert(txid, module);
    }
}
