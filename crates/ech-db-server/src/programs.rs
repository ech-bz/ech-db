use std::collections::HashMap;
use std::sync::Mutex;

use wasmi::{Engine, Module};

use ech_db_protocol::error::Reject;
use ech_db_protocol::id::{Root, Txid};
use ech_db_protocol::value::{Invocation, Program};

use crate::abi::Abi;
use crate::error::DbError;
use crate::fdb::Reader;
use crate::log::Log;

pub struct Programs {
    engine: Engine,
    cache: Mutex<HashMap<Txid, Module>>,
}

impl Programs {
    pub const UPLOAD_PROGRAM: &'static str = "upload_program";

    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn native(&self, txid: &Txid, invocation: &Invocation) -> Result<Vec<u8>, DbError> {
        match invocation.call.function.as_str() {
            Self::UPLOAD_PROGRAM => self.upload(txid, &invocation.call.args),
            function => Err(DbError::reject(Reject::UnknownNative {
                function: function.to_string(),
            })),
        }
    }

    pub async fn module<R: Reader>(
        &self,
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<Module, DbError> {
        if !Log::exists(access, root, txid).await? {
            return Err(DbError::reject(Reject::ProgramNotFound));
        }
        if let Some(module) = self.get(txid) {
            return Ok(module);
        }
        let wasm = self.wasm(access, root, txid).await?;
        let module = Module::new(&self.engine, wasm.as_slice())
            .map_err(|_| DbError::reject(Reject::InvalidWasm))?;
        Abi::check(&module)?;
        self.cache(*txid, module.clone());
        Ok(module)
    }

    fn upload(&self, txid: &Txid, wasm: &[u8]) -> Result<Vec<u8>, DbError> {
        let module =
            Module::new(&self.engine, wasm).map_err(|_| DbError::reject(Reject::InvalidWasm))?;
        Abi::check(&module)?;
        self.cache(*txid, module);
        Ok(Vec::new())
    }

    async fn wasm<R: Reader>(
        &self,
        access: &R,
        root: &Root,
        txid: &Txid,
    ) -> Result<Vec<u8>, DbError> {
        let tx = Log::tx(access, root, txid).await?;
        if !matches!(tx.call.program, Program::Native) || tx.call.function != Self::UPLOAD_PROGRAM {
            return Err(DbError::reject(Reject::NotAProgram));
        }
        Ok(tx.call.args)
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
