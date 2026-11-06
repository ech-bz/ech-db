use bcs::{from_bytes, to_bytes};
use futures::{stream, StreamExt, TryStreamExt};
use tracing::Instrument;
use wasmi::errors::ErrorKind;
use wasmi::{
    Caller, Engine, Linker, Memory, Module, Store, TrapCode, TypedResumableCall,
    TypedResumableCallHostTrap, Val,
};

use ech_db_protocol::error::Reject;
use ech_db_protocol::id::Root;
use ech_db_protocol::keys::{Range, ReadKey};
use ech_db_protocol::value::{CallOutput, Request};

use crate::error::DbError;
use crate::fdb::{Mutator, Reader};
use crate::timing::Timer;
use crate::wasm::{Profile, ProfileLimiter};

const SUSPEND: i32 = 0x0ECDB;
const CONCURRENT_READS: usize = 64;

pub struct Executor {
    engine: Engine,
}

impl Executor {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }

    pub async fn run<R: Reader>(
        &self,
        access: &R,
        module: &Module,
        root: &Root,
        request: &Request,
    ) -> Result<Vec<u8>, DbError> {
        let _execute = Timer::new("db_execute");
        let setup = Timer::new("db_setup");
        let (mut store, entry, ptr, length) = tracing::debug_span!(target: "latency", "wasm.setup")
            .in_scope(|| {
                let mut linker = <Linker<ExecState>>::new(&self.engine);
                linker
                    .func_wrap("ech", "get", Host::get)
                    .map_err(|_| DbError::reject(Reject::Abi))?;
                linker
                    .func_wrap("ech", "get_range", Host::get_range)
                    .map_err(|_| DbError::reject(Reject::Abi))?;
                linker
                    .func_wrap("ech", "response_copy", Host::response_copy)
                    .map_err(|_| DbError::reject(Reject::Abi))?;
                linker
                    .func_wrap("ech", "set", Host::set)
                    .map_err(|_| DbError::reject(Reject::Abi))?;
                linker
                    .func_wrap("ech", "del", Host::del)
                    .map_err(|_| DbError::reject(Reject::Abi))?;
                linker
                    .func_wrap("ech", "abort", Host::abort)
                    .map_err(|_| DbError::reject(Reject::Abi))?;
                linker
                    .func_wrap("ech", "read_only", Host::read_only)
                    .map_err(|_| DbError::reject(Reject::Abi))?;
                let mut store = Store::new(&self.engine, ExecState::new(*root, access.read_only()));
                store.limiter(|state| &mut state.limits);
                store
                    .set_fuel(Profile::FUEL)
                    .map_err(|_| DbError::reject(Reject::VmTrap))?;
                let instance = linker
                    .instantiate_and_start(&mut store, module)
                    .map_err(|_| DbError::reject(Reject::InvalidWasm))?;
                let memory = instance
                    .get_memory(&store, "memory")
                    .ok_or_else(|| DbError::reject(Reject::Abi))?;
                store.data_mut().memory = Some(memory);
                let alloc = instance
                    .get_typed_func::<i32, i32>(&store, "ech_alloc")
                    .map_err(|_| DbError::reject(Reject::Abi))?;
                let entry = instance
                    .get_typed_func::<(i32, i32), i64>(&store, "ech_call")
                    .map_err(|_| DbError::reject(Reject::Abi))?;
                let payload =
                    to_bytes(&(root, request)).map_err(|_| DbError::reject(Reject::Abi))?;
                let length = i32::try_from(payload.len())
                    .map_err(|_| DbError::reject(Reject::ResourceLimit))?;
                store.data_mut().in_alloc = true;
                let ptr = match alloc.call(&mut store, length) {
                    Ok(ptr) => ptr,
                    Err(error) => {
                        store.data_mut().in_alloc = false;
                        return Err(Self::trap(&mut store, &error));
                    }
                };
                store.data_mut().in_alloc = false;
                Self::write(&mut store, ptr, &payload)?;
                Ok::<_, DbError>((store, entry, ptr, length))
            })?;
        drop(setup);
        let call = Timer::new("db_wasm");
        let mut step = match tracing::debug_span!(target: "latency", "wasm.call")
            .in_scope(|| entry.call_resumable(&mut store, (ptr, length)))
        {
            Ok(step) => step,
            Err(error) => return Err(Self::trap(&mut store, &error)),
        };
        drop(call);
        let value = loop {
            match step {
                TypedResumableCall::Finished(value) => break value,
                TypedResumableCall::OutOfFuel(_) => return Err(DbError::reject(Reject::OutOfFuel)),
                TypedResumableCall::HostTrap(trap) => {
                    if let Some(fatal) = store.data_mut().fatal.take() {
                        return Err(fatal);
                    }
                    let pending = match store.data_mut().pending.take() {
                        Some(pending) => pending,
                        None => return Err(DbError::reject(Reject::Abi)),
                    };
                    step = Self::resume(access, &mut store, trap, pending).await?;
                }
            }
        };
        let _decode = Timer::new("db_output_decode");
        match tracing::debug_span!(target: "latency", "wasm.decode_output")
            .in_scope(|| Self::output(&store, value))?
        {
            CallOutput::Ok(bytes) => Ok(bytes),
            CallOutput::Err(bytes) => Err(DbError::reject(Reject::Application { bytes })),
        }
    }

    async fn resume<R: Reader>(
        access: &R,
        store: &mut Store<ExecState>,
        typed: TypedResumableCallHostTrap<i64>,
        pending: Pending,
    ) -> Result<TypedResumableCall<i64>, DbError> {
        let root = store.data().root;
        match pending {
            Pending::Get(keys) => {
                let read = Timer::new("db_get");
                let reads: Vec<_> = keys
                    .into_iter()
                    .map(|key| {
                        let key = key.physical(&root);
                        async move { access.get(&key).await }
                    })
                    .collect();
                let count = reads.len();
                let values: Vec<Option<Vec<u8>>> = stream::iter(reads)
                    .buffered(CONCURRENT_READS)
                    .try_collect()
                    .instrument(
                        tracing::debug_span!(target: "latency", "fdb.get_batch", keys = count,
                        concurrency = CONCURRENT_READS),
                    )
                    .await?;
                drop(read);
                let encode = Timer::new("db_host_encode");
                let bytes =
                    tracing::debug_span!(target: "latency", "host.encode_get", keys = count)
                        .in_scope(|| {
                            let values: Vec<_> = values
                                .iter()
                                .map(|value| value.as_deref().map(serde_bytes::Bytes::new))
                                .collect();
                            to_bytes(&values)
                        })
                        .map_err(|_| DbError::integrity())?;
                Self::charge(store, bytes.len() as u64)?;
                let length = i32::try_from(bytes.len())
                    .map_err(|_| DbError::reject(Reject::ResourceLimit))?;
                store.data_mut().response = Some(bytes);
                drop(encode);
                Self::resume_wasm(store, typed, &[Val::I32(length)])
            }
            Pending::Range(range) => {
                let read = Timer::new("db_range");
                let (begin, end) = range.physical(&root);
                let prefix = begin.len() - range.start.len();
                let rows = access
                    .scan(&begin, &end, None)
                    .instrument(tracing::debug_span!(target: "latency", "fdb.range"))
                    .await?;
                drop(read);
                let encode = Timer::new("db_host_encode");
                let bytes =
                    tracing::debug_span!(target: "latency", "host.encode_range", rows = rows.len())
                        .in_scope(|| {
                            let mut values = Vec::with_capacity(rows.len());
                            for (key, value) in &rows {
                                let suffix = key.get(prefix..).ok_or_else(DbError::integrity)?;
                                values.push((
                                    serde_bytes::Bytes::new(suffix),
                                    serde_bytes::Bytes::new(value),
                                ));
                            }
                            to_bytes(&values).map_err(|_| DbError::integrity())
                        })?;
                Self::charge(store, bytes.len() as u64)?;
                let length = i32::try_from(bytes.len())
                    .map_err(|_| DbError::reject(Reject::ResourceLimit))?;
                store.data_mut().response = Some(bytes);
                drop(encode);
                Self::resume_wasm(store, typed, &[Val::I32(length)])
            }
            Pending::Set(user_key, value) => {
                let key = ReadKey::kv(user_key).physical(&root);
                let total = store
                    .data()
                    .writes
                    .checked_add(key.len() + value.len())
                    .ok_or_else(|| DbError::reject(Reject::ResourceLimit))?;
                if total > Profile::WRITE_LIMIT {
                    return Err(DbError::reject(Reject::ResourceLimit));
                }
                access.writes().put(&key, &value)?;
                store.data_mut().writes = total;
                Self::resume_wasm(store, typed, &[])
            }
            Pending::Del(user_key) => {
                let key = ReadKey::kv(user_key).physical(&root);
                access.writes().clear(&key)?;
                Self::resume_wasm(store, typed, &[])
            }
            Pending::Abort(bytes) => Err(DbError::reject(Reject::Application { bytes })),
        }
    }

    #[tracing::instrument(target = "latency", level = "debug", name = "wasm.resume", skip_all,
        fields(response_bytes = store.data().response.as_ref().map_or(0, Vec::len)))]
    fn resume_wasm(
        store: &mut Store<ExecState>,
        typed: TypedResumableCallHostTrap<i64>,
        values: &[Val],
    ) -> Result<TypedResumableCall<i64>, DbError> {
        let _resume = Timer::new("db_wasm");
        match typed.resume(&mut *store, values) {
            Ok(next) => Ok(next),
            Err(error) => Err(Self::trap(store, &error)),
        }
    }

    fn trap(store: &mut Store<ExecState>, error: &wasmi::Error) -> DbError {
        if let Some(fatal) = store.data_mut().fatal.take() {
            return fatal;
        }
        match error.kind() {
            ErrorKind::TrapCode(TrapCode::OutOfFuel) => DbError::reject(Reject::OutOfFuel),
            ErrorKind::TrapCode(TrapCode::GrowthOperationLimited) => {
                DbError::reject(Reject::ResourceLimit)
            }
            _ => DbError::reject(Reject::VmTrap),
        }
    }

    fn charge(store: &mut Store<ExecState>, fuel: u64) -> Result<(), DbError> {
        let remaining = store
            .get_fuel()
            .map_err(|_| DbError::reject(Reject::VmTrap))?;
        if remaining < fuel {
            return Err(DbError::reject(Reject::OutOfFuel));
        }
        store
            .set_fuel(remaining - fuel)
            .map_err(|_| DbError::reject(Reject::VmTrap))
    }

    fn write(store: &mut Store<ExecState>, ptr: i32, bytes: &[u8]) -> Result<(), DbError> {
        let memory = store
            .data()
            .memory
            .ok_or_else(|| DbError::reject(Reject::Abi))?;
        let offset = ptr as u32 as usize;
        let end = offset
            .checked_add(bytes.len())
            .ok_or_else(|| DbError::reject(Reject::Abi))?;
        if end > memory.data_size(&*store) {
            return Err(DbError::reject(Reject::Abi));
        }
        memory
            .write(store, offset, bytes)
            .map_err(|_| DbError::reject(Reject::Abi))
    }

    fn output(store: &Store<ExecState>, value: i64) -> Result<CallOutput, DbError> {
        let ptr = ((value as u64) >> 32) as u32 as usize;
        let len = ((value as u64) & 0xFFFF_FFFF) as u32 as usize;
        let memory = store
            .data()
            .memory
            .ok_or_else(|| DbError::reject(Reject::Abi))?;
        let end = ptr
            .checked_add(len)
            .ok_or_else(|| DbError::reject(Reject::Abi))?;
        if end > memory.data_size(store) {
            return Err(DbError::reject(Reject::Abi));
        }
        if len > Profile::RESPONSE_LIMIT {
            return Err(DbError::reject(Reject::ResourceLimit));
        }
        from_bytes(&memory.data(store)[ptr..end]).map_err(|_| DbError::reject(Reject::Abi))
    }
}

struct ExecState {
    root: Root,
    read_only: bool,
    memory: Option<Memory>,
    in_alloc: bool,
    response: Option<Vec<u8>>,
    pending: Option<Pending>,
    fatal: Option<DbError>,
    writes: usize,
    limits: ProfileLimiter,
}

impl ExecState {
    fn new(root: Root, read_only: bool) -> Self {
        Self {
            root,
            read_only,
            memory: None,
            in_alloc: false,
            response: None,
            pending: None,
            fatal: None,
            writes: 0,
            limits: Profile::limits(),
        }
    }
}

enum Pending {
    Get(Vec<ReadKey>),
    Range(Range),
    Set(Vec<u8>, Vec<u8>),
    Del(Vec<u8>),
    Abort(Vec<u8>),
}

struct Host;

impl Host {
    fn get(mut caller: Caller<'_, ExecState>, ptr: i32, len: i32) -> Result<i32, wasmi::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            from_bytes::<Vec<ReadKey>>(bytes)
                .map(Pending::Get)
                .map_err(|_| DbError::reject(Reject::Abi))
        })
    }

    fn get_range(
        mut caller: Caller<'_, ExecState>,
        ptr: i32,
        len: i32,
    ) -> Result<i32, wasmi::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            from_bytes::<Range>(bytes)
                .map(Pending::Range)
                .map_err(|_| DbError::reject(Reject::Abi))
        })
    }

    fn set(mut caller: Caller<'_, ExecState>, ptr: i32, len: i32) -> Result<(), wasmi::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            from_bytes::<(Vec<u8>, Vec<u8>)>(bytes)
                .map(|(key, value)| Pending::Set(key, value))
                .map_err(|_| DbError::reject(Reject::Abi))
        })
        .map(|_| ())
    }

    fn del(mut caller: Caller<'_, ExecState>, ptr: i32, len: i32) -> Result<(), wasmi::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            from_bytes::<Vec<u8>>(bytes)
                .map(Pending::Del)
                .map_err(|_| DbError::reject(Reject::Abi))
        })
        .map(|_| ())
    }

    fn abort(mut caller: Caller<'_, ExecState>, ptr: i32, len: i32) -> Result<(), wasmi::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            Ok(Pending::Abort(bytes.to_vec()))
        })
        .map(|_| ())
    }

    fn read_only(caller: Caller<'_, ExecState>) -> Result<i32, wasmi::Error> {
        Ok(i32::from(caller.data().read_only))
    }

    fn response_copy(
        mut caller: Caller<'_, ExecState>,
        ptr: i32,
        capacity: i32,
    ) -> Result<i32, wasmi::Error> {
        if caller.data().in_alloc {
            return Err(Self::fail(&mut caller, DbError::reject(Reject::Abi)));
        }
        let capacity = capacity as u32 as usize;
        let Some(length) = caller.data().response.as_ref().map(Vec::len) else {
            return Err(Self::fail(&mut caller, DbError::reject(Reject::Abi)));
        };
        if capacity < length {
            return Err(Self::fail(&mut caller, DbError::reject(Reject::Abi)));
        }
        let cost = Profile::HOST_CALL_BASE_FUEL + 2 * length as u64;
        if let Err(error) = Self::charge(&mut caller, cost) {
            return Err(Self::fail(&mut caller, error));
        }
        let memory = match caller.data().memory {
            Some(memory) => memory,
            None => return Err(Self::fail(&mut caller, DbError::reject(Reject::Abi))),
        };
        let offset = ptr as u32 as usize;
        let end = match offset.checked_add(length) {
            Some(end) => end,
            None => return Err(Self::fail(&mut caller, DbError::reject(Reject::Abi))),
        };
        if end > memory.data_size(&caller) {
            return Err(Self::fail(&mut caller, DbError::reject(Reject::Abi)));
        }
        let (buffer, state) = memory.data_and_store_mut(&mut caller);
        let response = state.response.as_deref().expect("response exists");
        buffer[offset..end].copy_from_slice(response);
        i32::try_from(length).map_err(|_| Self::fail(&mut caller, DbError::reject(Reject::Abi)))
    }

    fn request(
        caller: &mut Caller<'_, ExecState>,
        ptr: i32,
        len: i32,
        parse: impl FnOnce(&[u8]) -> Result<Pending, DbError>,
    ) -> Result<i32, wasmi::Error> {
        if caller.data().in_alloc {
            return Err(Self::fail(caller, DbError::reject(Reject::Abi)));
        }
        let input_len = len as u32 as usize;
        let input = match Self::read(caller, ptr, len) {
            Ok(bytes) => bytes,
            Err(error) => return Err(Self::fail(caller, error)),
        };
        let cost = Profile::HOST_CALL_BASE_FUEL + input_len as u64;
        if let Err(error) = Self::charge(caller, cost) {
            return Err(Self::fail(caller, error));
        }
        let pending = match parse(&input) {
            Ok(pending) => pending,
            Err(error) => return Err(Self::fail(caller, error)),
        };
        caller.data_mut().pending = Some(pending);
        Err(wasmi::Error::i32_exit(SUSPEND))
    }

    fn read(caller: &Caller<'_, ExecState>, ptr: i32, len: i32) -> Result<Vec<u8>, DbError> {
        let memory = caller
            .data()
            .memory
            .ok_or_else(|| DbError::reject(Reject::Abi))?;
        let offset = ptr as u32 as usize;
        let length = len as u32 as usize;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| DbError::reject(Reject::Abi))?;
        if end > memory.data_size(caller) {
            return Err(DbError::reject(Reject::Abi));
        }
        let mut buffer = vec![0u8; length];
        memory
            .read(caller, offset, &mut buffer)
            .map_err(|_| DbError::reject(Reject::Abi))?;
        Ok(buffer)
    }

    fn charge(caller: &mut Caller<'_, ExecState>, fuel: u64) -> Result<(), DbError> {
        let remaining = caller
            .get_fuel()
            .map_err(|_| DbError::reject(Reject::VmTrap))?;
        if remaining < fuel {
            return Err(DbError::reject(Reject::OutOfFuel));
        }
        caller
            .set_fuel(remaining - fuel)
            .map_err(|_| DbError::reject(Reject::VmTrap))
    }

    fn fail(caller: &mut Caller<'_, ExecState>, error: DbError) -> wasmi::Error {
        caller.data_mut().fatal = Some(error);
        wasmi::Error::i32_exit(SUSPEND)
    }
}
