use wasmi::{ExternType, Module, ValType};

use ech_db_protocol::error::Reject;

use crate::error::DbError;

pub struct Abi;

impl Abi {
    pub const MODULE: &'static str = "ech";

    pub fn check(module: &Module) -> Result<(), DbError> {
        for import in module.imports() {
            if import.module() != Self::MODULE {
                return Err(DbError::reject(Reject::Abi));
            }
            let ExternType::Func(func) = import.ty() else {
                return Err(DbError::reject(Reject::Abi));
            };
            let (params, results) = match import.name() {
                "map_get" | "map_range" | "tx_get" | "response_copy" => (2, 1),
                "map_insert" | "map_remove" | "abort" => (2, 0),
                "read_only" => (0, 1),
                _ => return Err(DbError::reject(Reject::Abi)),
            };
            if func.params().len() != params || func.results().len() != results {
                return Err(DbError::reject(Reject::Abi));
            }
            if !func.params().iter().all(|param| *param == ValType::I32) {
                return Err(DbError::reject(Reject::Abi));
            }
            if results == 1 && func.results()[0] != ValType::I32 {
                return Err(DbError::reject(Reject::Abi));
            }
        }
        match module.get_export("memory") {
            Some(ExternType::Memory(_)) => {}
            _ => return Err(DbError::reject(Reject::Abi)),
        }
        Self::check_export(module, "ech_alloc", 1, ValType::I32)?;
        Self::check_export(module, "ech_call", 2, ValType::I64)?;
        Ok(())
    }

    fn check_export(
        module: &Module,
        name: &str,
        params: usize,
        result: ValType,
    ) -> Result<(), DbError> {
        match module.get_export(name) {
            Some(ExternType::Func(func))
                if func.params().len() == params
                    && func.params().iter().all(|param| *param == ValType::I32)
                    && func.results().len() == 1
                    && func.results()[0] == result =>
            {
                Ok(())
            }
            _ => Err(DbError::reject(Reject::Abi)),
        }
    }
}
