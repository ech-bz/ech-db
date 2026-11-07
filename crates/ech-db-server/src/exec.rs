use std::time::{Duration, Instant};

use bcs::{from_bytes, to_bytes};
use futures::channel::{mpsc, oneshot};
use futures::{stream, StreamExt, TryStreamExt};
use tracing::Instrument;
use wasmtime::{Caller, Engine, Linker, Memory, Module, Store, Trap};

use ech_db_protocol::error::Reject;
use ech_db_protocol::id::{Root, Txid};
use ech_db_protocol::map::{MapKey, MapRange};
use ech_db_protocol::value::{CallOutput, Request};

use crate::big_map::BigMap;
use crate::error::DbError;
use crate::fdb::{Mutator, Reader};
use crate::log::Log;
use crate::timing::{self, Timer};
use crate::wasm::{Profile, ProfileLimiter};

const CONCURRENT_READS: usize = 64;

pub struct Executor {
    engine: Engine,
    linker: Linker<ExecState>,
}

impl Executor {
    pub fn new(engine: Engine) -> Result<Self, wasmtime::Error> {
        let mut linker = Linker::new(&engine);
        linker.func_wrap_async("ech", "map_get", |caller, (ptr, len)| {
            Box::new(Host::map_get(caller, ptr, len))
        })?;
        linker.func_wrap_async("ech", "map_range", |caller, (ptr, len)| {
            Box::new(Host::map_range(caller, ptr, len))
        })?;
        linker.func_wrap_async("ech", "tx_get", |caller, (ptr, len)| {
            Box::new(Host::tx_get(caller, ptr, len))
        })?;
        linker.func_wrap_async("ech", "map_insert", |caller, (ptr, len)| {
            Box::new(Host::map_insert(caller, ptr, len))
        })?;
        linker.func_wrap_async("ech", "map_remove", |caller, (ptr, len)| {
            Box::new(Host::map_remove(caller, ptr, len))
        })?;
        linker.func_wrap_async("ech", "abort", |caller, (ptr, len)| {
            Box::new(Host::abort(caller, ptr, len))
        })?;
        linker.func_wrap("ech", "response_copy", Host::response_copy)?;
        linker.func_wrap("ech", "read_only", Host::read_only)?;
        linker.func_wrap("ech", "profile_enter", Host::profile_enter)?;
        linker.func_wrap("ech", "profile_exit", Host::profile_exit)?;
        Ok(Self { engine, linker })
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
        let (requests, mut pending) = mpsc::unbounded();
        let mut store = Store::new(&self.engine, ExecState::new(access.read_only(), requests));
        store.limiter(|state| &mut state.limits);
        store
            .set_fuel(Profile::FUEL)
            .map_err(|_| DbError::reject(Reject::VmTrap))?;
        let instance = self
            .linker
            .instantiate_async(&mut store, module)
            .instrument(tracing::debug_span!(target: "latency", "wasm.setup"))
            .await
            .map_err(|error| Self::trap(&mut store, error))?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| DbError::reject(Reject::Abi))?;
        store.data_mut().memory = Some(memory);
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "ech_alloc")
            .map_err(|_| DbError::reject(Reject::Abi))?;
        let entry = instance
            .get_typed_func::<(i32, i32), i64>(&mut store, "ech_call")
            .map_err(|_| DbError::reject(Reject::Abi))?;
        let payload = to_bytes(&(root, request)).map_err(|_| DbError::reject(Reject::Abi))?;
        let length =
            i32::try_from(payload.len()).map_err(|_| DbError::reject(Reject::ResourceLimit))?;
        store.data_mut().in_alloc = true;
        let allocated = alloc.call_async(&mut store, length).await;
        store.data_mut().in_alloc = false;
        let ptr = allocated.map_err(|error| Self::trap(&mut store, error))?;
        Self::write(&mut store, ptr, &payload)?;
        drop(setup);
        let fuel = store
            .get_fuel()
            .map_err(|_| DbError::reject(Reject::VmTrap))?;
        store.data_mut().segment = Some(WasmSegment::new(fuel, payload.len()));
        let mut writes = 0;
        let result = {
            let call = entry.call_async(&mut store, (ptr, length));
            tokio::pin!(call);
            loop {
                tokio::select! {
                    result = &mut call => break result,
                    operation = pending.next() => {
                        let Some(Operation { request, reply }) = operation else {
                            return Err(DbError::integrity());
                        };
                        let result = Self::perform(access, root, &mut writes, request).await;
                        if reply.send(result).is_err() {
                            return Err(DbError::integrity());
                        }
                    }
                }
            }
        };
        let remaining = store
            .get_fuel()
            .map_err(|_| DbError::reject(Reject::VmTrap))?;
        if let Some(segment) = store.data_mut().segment.take() {
            segment.finish(remaining, if result.is_ok() { "finished" } else { "trap" });
        }
        let value = result.map_err(|error| Self::trap(&mut store, error))?;
        let _decode = Timer::new("db_output_decode");
        match tracing::debug_span!(target: "latency", "wasm.decode_output")
            .in_scope(|| Self::output(&store, value))?
        {
            CallOutput::Ok(bytes) => Ok(bytes),
            CallOutput::Err(bytes) => Err(DbError::reject(Reject::Application { bytes })),
        }
    }

    async fn perform<R: Reader>(
        access: &R,
        root: &Root,
        writes: &mut usize,
        pending: Pending,
    ) -> Result<Option<Vec<u8>>, DbError> {
        match pending {
            Pending::Get(keys) => {
                let started = Instant::now();
                let read = Timer::new("db_get");
                let reads: Vec<_> = keys
                    .into_iter()
                    .map(|key| {
                        let key = BigMap::physical(root, &key);
                        async move {
                            let issued = Instant::now();
                            let value = access.get(&key).await?;
                            Ok::<_, DbError>((
                                value,
                                issued.elapsed(),
                                issued.duration_since(started),
                            ))
                        }
                    })
                    .collect();
                let count = reads.len();
                let (values, total, longest, last_issued) = async {
                    let mut reads = stream::iter(reads).buffered(CONCURRENT_READS);
                    let mut values = Vec::with_capacity(count);
                    let mut total = Duration::ZERO;
                    let mut longest = Duration::ZERO;
                    let mut last_issued = Duration::ZERO;
                    while let Some((value, elapsed, issued)) = reads.try_next().await? {
                        total += elapsed;
                        longest = longest.max(elapsed);
                        last_issued = last_issued.max(issued);
                        values.push(value);
                    }
                    Ok::<_, DbError>((values, total, longest, last_issued))
                }
                .instrument(
                    tracing::debug_span!(target: "latency", "fdb.get_batch", keys = count,
                        concurrency = CONCURRENT_READS),
                )
                .await?;
                let found = values.iter().flatten().count();
                let bytes: usize = values.iter().flatten().map(Vec::len).sum();
                read.finish(format!(
                    "keys={count} found={found} bytes={bytes} concurrency={CONCURRENT_READS} avg_ms={:.3} max_ms={:.3} last_issue_ms={:.3}",
                    total.as_secs_f64() * 1000.0 / count.max(1) as f64,
                    longest.as_secs_f64() * 1000.0,
                    last_issued.as_secs_f64() * 1000.0,
                ));
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
                let length = i32::try_from(bytes.len())
                    .map_err(|_| DbError::reject(Reject::ResourceLimit))?;
                encode.finish(format!("get keys={count} bytes={length}"));
                Ok(Some(bytes))
            }
            Pending::Range(range) => {
                let read = Timer::new("db_range");
                let map = BigMap::open(root, &range.map);
                let (begin, end) = map.bounds(&range);
                let rows = if range.limit == 0 || begin >= end {
                    Vec::new()
                } else {
                    access
                        .scan(&begin, &end, Some(u64::from(range.limit)))
                        .instrument(tracing::debug_span!(target: "latency", "fdb.range"))
                        .await?
                };
                let row_bytes: usize = rows
                    .iter()
                    .map(|(key, value)| key.len() + value.len())
                    .sum();
                read.finish(format!(
                    "limit={} rows={} bytes={row_bytes}",
                    range.limit,
                    rows.len()
                ));
                let encode = Timer::new("db_host_encode");
                let bytes =
                    tracing::debug_span!(target: "latency", "host.encode_range", rows = rows.len())
                        .in_scope(|| {
                            let mut values = Vec::with_capacity(rows.len());
                            for (key, value) in &rows {
                                let suffix = map.logical(key).ok_or_else(DbError::integrity)?;
                                values.push((
                                    serde_bytes::Bytes::new(suffix),
                                    serde_bytes::Bytes::new(value),
                                ));
                            }
                            to_bytes(&values).map_err(|_| DbError::integrity())
                        })?;
                let length = i32::try_from(bytes.len())
                    .map_err(|_| DbError::reject(Reject::ResourceLimit))?;
                encode.finish(format!("range rows={} bytes={length}", rows.len()));
                Ok(Some(bytes))
            }
            Pending::Set(user_key, value) => {
                let key = BigMap::physical(root, &user_key);
                let total = writes
                    .checked_add(key.len() + value.len())
                    .ok_or_else(|| DbError::reject(Reject::ResourceLimit))?;
                if total > Profile::WRITE_LIMIT {
                    return Err(DbError::reject(Reject::ResourceLimit));
                }
                access.writes().put(&key, &value)?;
                *writes = total;
                Ok(None)
            }
            Pending::Del(user_key) => {
                let key = BigMap::physical(root, &user_key);
                access.writes().clear(&key)?;
                Ok(None)
            }
            Pending::Tx(txid) => {
                let read = Timer::new("db_tx");
                let bytes = Log::body(access, root, &txid).await?;
                drop(read);
                Ok(Some(bytes))
            }
            Pending::Abort(bytes) => Err(DbError::reject(Reject::Application { bytes })),
        }
    }

    fn trap(store: &mut Store<ExecState>, error: wasmtime::Error) -> DbError {
        if let Some(fatal) = store.data_mut().fatal.take() {
            return fatal;
        }
        if matches!(error.downcast_ref::<Trap>(), Some(Trap::OutOfFuel)) {
            return DbError::reject(Reject::OutOfFuel);
        }
        error
            .downcast::<DbError>()
            .unwrap_or_else(|_| DbError::reject(Reject::VmTrap))
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
    read_only: bool,
    memory: Option<Memory>,
    in_alloc: bool,
    response: Option<Vec<u8>>,
    requests: mpsc::UnboundedSender<Operation>,
    segment: Option<WasmSegment>,
    fatal: Option<DbError>,
    limits: ProfileLimiter,
    profile: Vec<ProfileSpan>,
}

struct ProfileSpan {
    name: String,
    started: Instant,
    fuel: u64,
}

impl ExecState {
    fn new(read_only: bool, requests: mpsc::UnboundedSender<Operation>) -> Self {
        Self {
            read_only,
            memory: None,
            in_alloc: false,
            response: None,
            requests,
            segment: None,
            fatal: None,
            limits: Profile::limits(),
            profile: Vec::new(),
        }
    }
}

struct WasmSegment {
    timer: Timer,
    fuel: u64,
    bytes: usize,
}

impl WasmSegment {
    fn new(fuel: u64, bytes: usize) -> Self {
        Self {
            timer: Timer::new("db_wasm"),
            fuel,
            bytes,
        }
    }

    fn finish(self, remaining: u64, next: &str) {
        self.timer.finish(format!(
            "input_bytes={} fuel={} next={next}",
            self.bytes,
            self.fuel.saturating_sub(remaining),
        ));
    }
}

struct Operation {
    request: Pending,
    reply: oneshot::Sender<Result<Option<Vec<u8>>, DbError>>,
}

enum Pending {
    Get(Vec<MapKey>),
    Range(MapRange),
    Set(MapKey, Vec<u8>),
    Del(MapKey),
    Tx(Txid),
    Abort(Vec<u8>),
}

struct Host;

impl Host {
    fn profile_enter(
        mut caller: Caller<'_, ExecState>,
        ptr: i32,
        len: i32,
    ) -> Result<(), wasmtime::Error> {
        if caller.data().in_alloc || !(1..=32).contains(&len) || caller.data().profile.len() >= 16 {
            return Err(Self::fail(&mut caller, DbError::reject(Reject::Abi)));
        }
        if let Err(error) = Self::charge(&mut caller, Profile::HOST_CALL_BASE_FUEL) {
            return Err(Self::fail(&mut caller, error));
        }
        let name = Self::read(&caller, ptr, len)
            .and_then(|bytes| {
                if !bytes
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
                {
                    return Err(DbError::reject(Reject::Abi));
                }
                String::from_utf8(bytes).map_err(|_| DbError::reject(Reject::Abi))
            })
            .map_err(|error| Self::fail(&mut caller, error))?;
        let fuel = caller.get_fuel()?;
        caller.data_mut().profile.push(ProfileSpan {
            name,
            started: Instant::now(),
            fuel,
        });
        Ok(())
    }

    fn profile_exit(mut caller: Caller<'_, ExecState>) -> Result<(), wasmtime::Error> {
        if caller.data().in_alloc {
            return Err(Self::fail(&mut caller, DbError::reject(Reject::Abi)));
        }
        let fuel = caller.get_fuel()?;
        if let Err(error) = Self::charge(&mut caller, Profile::HOST_CALL_BASE_FUEL) {
            return Err(Self::fail(&mut caller, error));
        }
        let Some(span) = caller.data_mut().profile.pop() else {
            return Err(Self::fail(&mut caller, DbError::reject(Reject::Abi)));
        };
        timing::guest(
            span.name,
            span.started.elapsed(),
            span.fuel.saturating_sub(fuel),
        );
        Ok(())
    }

    async fn map_get(
        mut caller: Caller<'_, ExecState>,
        ptr: i32,
        len: i32,
    ) -> Result<i32, wasmtime::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            from_bytes::<Vec<MapKey>>(bytes)
                .map(Pending::Get)
                .map_err(|_| DbError::reject(Reject::Abi))
        })
        .await
    }

    async fn map_range(
        mut caller: Caller<'_, ExecState>,
        ptr: i32,
        len: i32,
    ) -> Result<i32, wasmtime::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            from_bytes::<MapRange>(bytes)
                .map(Pending::Range)
                .map_err(|_| DbError::reject(Reject::Abi))
        })
        .await
    }

    async fn map_insert(
        mut caller: Caller<'_, ExecState>,
        ptr: i32,
        len: i32,
    ) -> Result<(), wasmtime::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            from_bytes::<(MapKey, serde_bytes::ByteBuf)>(bytes)
                .map(|(key, value)| Pending::Set(key, value.into_vec()))
                .map_err(|_| DbError::reject(Reject::Abi))
        })
        .await
        .map(|_| ())
    }

    async fn map_remove(
        mut caller: Caller<'_, ExecState>,
        ptr: i32,
        len: i32,
    ) -> Result<(), wasmtime::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            from_bytes::<MapKey>(bytes)
                .map(Pending::Del)
                .map_err(|_| DbError::reject(Reject::Abi))
        })
        .await
        .map(|_| ())
    }

    async fn tx_get(
        mut caller: Caller<'_, ExecState>,
        ptr: i32,
        len: i32,
    ) -> Result<i32, wasmtime::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            from_bytes::<Txid>(bytes)
                .map(Pending::Tx)
                .map_err(|_| DbError::reject(Reject::Abi))
        })
        .await
    }

    async fn abort(
        mut caller: Caller<'_, ExecState>,
        ptr: i32,
        len: i32,
    ) -> Result<(), wasmtime::Error> {
        Self::request(&mut caller, ptr, len, |bytes| {
            Ok(Pending::Abort(bytes.to_vec()))
        })
        .await
        .map(|_| ())
    }

    fn read_only(caller: Caller<'_, ExecState>) -> Result<i32, wasmtime::Error> {
        Ok(i32::from(caller.data().read_only))
    }

    fn response_copy(
        mut caller: Caller<'_, ExecState>,
        ptr: i32,
        capacity: i32,
    ) -> Result<i32, wasmtime::Error> {
        let _copy = Timer::new("db_host_copy");
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

    async fn request(
        caller: &mut Caller<'_, ExecState>,
        ptr: i32,
        len: i32,
        parse: impl FnOnce(&[u8]) -> Result<Pending, DbError>,
    ) -> Result<i32, wasmtime::Error> {
        let _request = Timer::new("db_host_request");
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
        drop(_request);
        let next = match &pending {
            Pending::Get(_) => "get",
            Pending::Range(_) => "range",
            Pending::Set(_, _) => "set",
            Pending::Del(_) => "del",
            Pending::Tx(_) => "tx",
            Pending::Abort(_) => "abort",
        };
        let fuel = caller.get_fuel()?;
        if let Some(segment) = caller.data_mut().segment.take() {
            segment.finish(fuel, next);
        }
        let (reply, response) = oneshot::channel();
        caller
            .data()
            .requests
            .unbounded_send(Operation {
                request: pending,
                reply,
            })
            .map_err(|_| Self::fail(caller, DbError::integrity()))?;
        let bytes = response
            .await
            .map_err(|_| Self::fail(caller, DbError::integrity()))?
            .map_err(|error| Self::fail(caller, error))?;
        let length = bytes.as_ref().map_or(0, Vec::len);
        if bytes.is_some() {
            Self::charge(caller, length as u64).map_err(|error| Self::fail(caller, error))?;
            caller.data_mut().response = bytes;
        }
        let length_i32 = i32::try_from(length)
            .map_err(|_| Self::fail(caller, DbError::reject(Reject::ResourceLimit)))?;
        let fuel = caller.get_fuel()?;
        caller.data_mut().segment = Some(WasmSegment::new(fuel, length));
        Ok(length_i32)
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

    fn fail(caller: &mut Caller<'_, ExecState>, error: DbError) -> wasmtime::Error {
        caller.data_mut().fatal = Some(error);
        wasmtime::Error::msg("guest host call failed")
    }
}
