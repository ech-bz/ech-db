extern crate self as ech_db_guest;

#[doc(hidden)]
pub use serde;

pub mod entry;

mod host;
pub use host::Host;

pub use ech_db_protocol::Bind;

mod big_log;
mod big_map;
mod big_set;
mod key;
pub use big_log::BigLog;
pub use big_map::BigMap;
pub use big_set::BigSet;
pub use key::Key;
pub type Result<T> = std::result::Result<T, Vec<u8>>;

pub use ech_db_protocol::error::{CodecError, Reject};
pub use ech_db_protocol::id::{Root, Txid};
pub use ech_db_protocol::proof::{Equation, Proof, Statement, Term};
pub use ech_db_protocol::value::{
    Author, Call, CallOutput, Decryption, Esid, Program, Request, ServerPart, Tx, UserPart,
};

pub fn load_transaction(txid: &Txid) -> Result<Tx> {
    bcs::from_bytes(&Host::tx_get(txid))
        .map_err(|error| format!("transaction body: {error}").into_bytes())
}
