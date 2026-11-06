#[cfg(target_arch = "wasm32")]
use serde::de::DeserializeOwned;
#[cfg(target_arch = "wasm32")]
use serde::Serialize;
#[cfg(target_arch = "wasm32")]
use serde_bytes::{ByteBuf, Bytes};

#[cfg(target_arch = "wasm32")]
use ech_db_protocol::id::Root;
#[cfg(target_arch = "wasm32")]
use ech_db_protocol::value::{CallOutput, Request};
use ech_db_protocol::{
    id::Txid,
    map::{MapKey, MapRange},
};

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "ech")]
extern "C" {
    fn map_get(ptr: i32, len: i32) -> i32;
    fn map_range(ptr: i32, len: i32) -> i32;
    fn tx_get(ptr: i32, len: i32) -> i32;
    fn response_copy(ptr: i32, capacity: i32) -> i32;
    fn map_insert(ptr: i32, len: i32);
    fn map_remove(ptr: i32, len: i32);
    fn abort(ptr: i32, len: i32);
    fn read_only() -> i32;
}

pub struct Host;

impl Host {
    #[cfg(target_arch = "wasm32")]
    pub fn read_only() -> bool {
        unsafe { read_only() != 0 }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn map_get(keys: &[MapKey]) -> Vec<Option<Vec<u8>>> {
        let input = to_bcs_bytes(keys);
        let response_len = unsafe { map_get(input.as_ptr() as i32, input.len() as i32) };
        let response = copy_response(response_len as u32 as usize);
        let values: Vec<Option<ByteBuf>> = from_bcs_bytes(&response);
        values
            .into_iter()
            .map(|value| value.map(ByteBuf::into_vec))
            .collect()
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn map_range(range: &MapRange) -> Vec<(Vec<u8>, Vec<u8>)> {
        let input = to_bcs_bytes(range);
        let response_len = unsafe { map_range(input.as_ptr() as i32, input.len() as i32) };
        let response = copy_response(response_len as u32 as usize);
        let rows: Vec<(ByteBuf, ByteBuf)> = from_bcs_bytes(&response);
        rows.into_iter()
            .map(|(key, value)| (key.into_vec(), value.into_vec()))
            .collect()
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn map_insert(key: &MapKey, value: &[u8]) {
        let input = to_bcs_bytes(&(key, Bytes::new(value)));
        unsafe { map_insert(input.as_ptr() as i32, input.len() as i32) };
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn map_remove(key: &MapKey) {
        let input = to_bcs_bytes(key);
        unsafe { map_remove(input.as_ptr() as i32, input.len() as i32) };
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn tx_get(txid: &Txid) -> Vec<u8> {
        let input = to_bcs_bytes(txid);
        let response_len = unsafe { tx_get(input.as_ptr() as i32, input.len() as i32) };
        copy_response(response_len as u32 as usize)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn abort(bytes: &[u8]) -> ! {
        unsafe { abort(bytes.as_ptr() as i32, bytes.len() as i32) };
        unreachable!("abort must not return")
    }
}

#[cfg(target_arch = "wasm32")]
fn copy_response(response_len: usize) -> Vec<u8> {
    let mut buffer = vec![0u8; response_len];
    let copied = unsafe { response_copy(buffer.as_mut_ptr() as i32, response_len as i32) };
    assert_eq!(copied as u32 as usize, response_len);
    buffer
}

#[cfg(target_arch = "wasm32")]
fn to_bcs_bytes<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    bcs::to_bytes(value).expect("bcs encode")
}

#[cfg(target_arch = "wasm32")]
fn from_bcs_bytes<T: DeserializeOwned>(bytes: &[u8]) -> T {
    bcs::from_bytes(bytes).expect("bcs decode")
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn decode_input(bytes: &[u8]) -> (Root, Request) {
    from_bcs_bytes(bytes)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn encode_call_output(output: &CallOutput) -> Vec<u8> {
    to_bcs_bytes(output)
}

#[cfg(not(target_arch = "wasm32"))]
impl Host {
    pub fn read_only() -> bool {
        unimplemented!("guest operations require the wasm runtime")
    }
    pub fn abort(_bytes: &[u8]) -> ! {
        unimplemented!("guest operations require the wasm runtime")
    }
    pub(crate) fn tx_get(_txid: &Txid) -> Vec<u8> {
        unimplemented!("guest operations require the wasm runtime")
    }
    pub(crate) fn map_get(_keys: &[MapKey]) -> Vec<Option<Vec<u8>>> {
        unimplemented!("guest operations require the wasm runtime")
    }
    pub(crate) fn map_range(_range: &MapRange) -> Vec<(Vec<u8>, Vec<u8>)> {
        unimplemented!("guest operations require the wasm runtime")
    }
    pub(crate) fn map_insert(_key: &MapKey, _value: &[u8]) {
        unimplemented!("guest operations require the wasm runtime")
    }
    pub(crate) fn map_remove(_key: &MapKey) {
        unimplemented!("guest operations require the wasm runtime")
    }
}
