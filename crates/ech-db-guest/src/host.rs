use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_bytes::{ByteBuf, Bytes};

use ech_db_protocol::id::Root;
use ech_db_protocol::keys::{Range, ReadKey};
use ech_db_protocol::value::{CallOutput, Request};

#[link(wasm_import_module = "ech")]
extern "C" {
    fn get(ptr: i32, len: i32) -> i32;
    fn get_range(ptr: i32, len: i32) -> i32;
    fn response_copy(ptr: i32, capacity: i32) -> i32;
    fn set(ptr: i32, len: i32);
    fn del(ptr: i32, len: i32);
    fn abort(ptr: i32, len: i32);
    fn read_only() -> i32;
}

pub struct Host;

impl Host {
    pub fn read_only() -> bool {
        unsafe { read_only() != 0 }
    }

    pub fn get_raw(keys: &[ReadKey]) -> Vec<Option<Vec<u8>>> {
        let input = to_bcs_bytes(keys);
        let response_len = unsafe { get(input.as_ptr() as i32, input.len() as i32) };
        let response = copy_response(response_len as u32 as usize);
        let values: Vec<Option<ByteBuf>> = from_bcs_bytes(&response);
        values
            .into_iter()
            .map(|value| value.map(ByteBuf::into_vec))
            .collect()
    }

    pub fn get<T: DeserializeOwned>(keys: &[ReadKey]) -> Vec<Option<T>> {
        Self::get_raw(keys)
            .into_iter()
            .map(|value| value.map(|bytes| from_bcs_bytes(&bytes)))
            .collect()
    }

    pub fn range_raw(range: &Range) -> Vec<(Vec<u8>, Vec<u8>)> {
        let input = to_bcs_bytes(range);
        let response_len = unsafe { get_range(input.as_ptr() as i32, input.len() as i32) };
        let response = copy_response(response_len as u32 as usize);
        let rows: Vec<(ByteBuf, ByteBuf)> = from_bcs_bytes(&response);
        rows.into_iter()
            .map(|(key, value)| (key.into_vec(), value.into_vec()))
            .collect()
    }

    pub fn range<T: DeserializeOwned>(range: &Range) -> Vec<(Vec<u8>, T)> {
        Self::range_raw(range)
            .into_iter()
            .map(|(key, value)| (key, from_bcs_bytes(&value)))
            .collect()
    }

    pub fn put(user_key: &[u8], value: &[u8]) {
        let input = to_bcs_bytes(&(Bytes::new(user_key), Bytes::new(value)));
        unsafe { set(input.as_ptr() as i32, input.len() as i32) };
    }

    pub fn put_value<T: Serialize>(user_key: &[u8], value: &T) {
        let value = to_bcs_bytes(value);
        Self::put(user_key, &value);
    }

    pub fn del(user_key: &[u8]) {
        let input = to_bcs_bytes(Bytes::new(user_key));
        unsafe { del(input.as_ptr() as i32, input.len() as i32) };
    }

    pub fn abort(bytes: &[u8]) -> ! {
        unsafe { abort(bytes.as_ptr() as i32, bytes.len() as i32) };
        unreachable!("abort must not return")
    }
}

fn copy_response(response_len: usize) -> Vec<u8> {
    let mut buffer = vec![0u8; response_len];
    let copied = unsafe { response_copy(buffer.as_mut_ptr() as i32, response_len as i32) };
    assert_eq!(copied as u32 as usize, response_len);
    buffer
}

fn to_bcs_bytes<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    bcs::to_bytes(value).expect("bcs encode")
}

fn from_bcs_bytes<T: DeserializeOwned>(bytes: &[u8]) -> T {
    bcs::from_bytes(bytes).expect("bcs decode")
}

pub fn decode_input(bytes: &[u8]) -> (Root, Request) {
    from_bcs_bytes(bytes)
}

pub fn encode_call_output(output: &CallOutput) -> Vec<u8> {
    to_bcs_bytes(output)
}
