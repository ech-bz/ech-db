use wasmtime::{Config, Engine, OptLevel, ResourceLimiter, Strategy, WasmFeatures};

use ech_db_protocol::error::Reject;

use crate::error::DbError;

pub struct Profile;

impl Profile {
    pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
    pub const TABLE_LIMIT: usize = 65_536;
    pub const STACK_LIMIT: usize = 1024 * 1024;
    pub const FUEL: u64 = 1_000_000_000;
    pub const HOST_CALL_BASE_FUEL: u64 = 100;
    pub const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
    pub const WRITE_LIMIT: usize = 8 * 1024 * 1024;

    pub fn engine() -> Result<Engine, wasmtime::Error> {
        let mut config = Config::default();
        config.strategy(Strategy::Cranelift);
        config.cranelift_opt_level(OptLevel::Speed);
        config.cranelift_nan_canonicalization(true);
        config.consume_fuel(true);
        config.max_wasm_stack(Self::STACK_LIMIT);
        config.async_stack_size(2 * Self::STACK_LIMIT);
        config.wasm_features(WasmFeatures::all(), false);
        config.wasm_features(
            WasmFeatures::WASM1
                | WasmFeatures::SIGN_EXTENSION
                | WasmFeatures::MULTI_VALUE
                | WasmFeatures::BULK_MEMORY
                | WasmFeatures::SIMD
                | WasmFeatures::REFERENCE_TYPES
                | WasmFeatures::GC_TYPES,
            true,
        );
        Engine::new(&config)
    }

    pub fn limits() -> ProfileLimiter {
        ProfileLimiter
    }
}

pub struct ProfileLimiter;

impl ResourceLimiter for ProfileLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        if desired > Profile::MEMORY_LIMIT {
            return Err(wasmtime::Error::new(DbError::reject(Reject::ResourceLimit)));
        }
        Ok(maximum.is_none_or(|max| desired <= max))
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        if desired > Profile::TABLE_LIMIT {
            return Err(wasmtime::Error::new(DbError::reject(Reject::ResourceLimit)));
        }
        Ok(maximum.is_none_or(|max| desired <= max))
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        10_000
    }

    fn memories(&self) -> usize {
        1
    }
}
