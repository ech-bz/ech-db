use crate::{host::Host, Bind, Key, Result};
use ech_db_protocol::map::{MapKey, MapRange};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fmt, marker::PhantomData};

mod batch;

#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
pub struct BigMap<K, V> {
    #[serde(skip)]
    name: Option<Vec<u8>>,
    #[serde(skip)]
    marker: PhantomData<fn(K) -> V>,
}

impl<K, V> BigMap<K, V> {
    pub fn get_batch<B: batch::BatchRead<Key = K, Value = V>>(batch: B) -> Result<B::Output> {
        batch::get_batch(batch)
    }

    pub fn new() -> Self {
        Self {
            name: None,
            marker: PhantomData,
        }
    }

    fn name(&self) -> Result<&[u8]> {
        self.name
            .as_deref()
            .ok_or_else(|| b"unbound BigMap".to_vec())
    }
}

impl<K, V> Bind for BigMap<K, V> {
    fn bind<N: Serialize + ?Sized>(&mut self, name: &N) {
        self.name = Some(bcs::to_bytes(name).expect("collection name"));
    }
}

impl<K, V> Default for BigMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}
impl<K, V> Clone for BigMap<K, V> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            marker: PhantomData,
        }
    }
}
impl<K, V> PartialEq for BigMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl<K, V> Eq for BigMap<K, V> {}
impl<K, V> fmt::Debug for BigMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BigMap")
    }
}

impl<K: Key, V> BigMap<K, V> {
    fn key(&self, key: &K) -> Result<MapKey> {
        Ok(MapKey {
            map: self.name()?.to_vec(),
            key: key.encode(),
        })
    }

    pub fn remove(&self, key: &K) -> Result<()> {
        Host::map_remove(&self.key(key)?);
        Ok(())
    }
}

impl<K: Key, V: Serialize> BigMap<K, V> {
    pub fn insert(&self, key: &K, value: &V) -> Result<()> {
        let bytes = bcs::to_bytes(value).map_err(|error| error.to_string().into_bytes())?;
        Host::map_insert(&self.key(key)?, &bytes);
        Ok(())
    }
}

impl<K: Key, V: DeserializeOwned> BigMap<K, V> {
    pub fn get(&self, key: &K) -> Result<Option<V>> {
        Self::get_batch((self, key))
    }

    pub fn get_many(&self, keys: &[K]) -> Result<Vec<Option<V>>> {
        Self::get_batch((self, keys))
    }

    pub fn entries(&self, after: Option<&K>, limit: usize) -> Result<Vec<(K, V)>> {
        self.range(after, None, limit)
    }

    pub fn range(&self, from: Option<&K>, to: Option<&K>, limit: usize) -> Result<Vec<(K, V)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let range = MapRange {
            map: self.name()?.to_vec(),
            after: from.map(Key::encode),
            before: to.map(Key::encode),
            limit: limit
                .try_into()
                .map_err(|_| b"BigMap range limit overflow".to_vec())?,
        };
        Host::map_range(&range)
            .into_iter()
            .map(|(key, value)| {
                let key = K::decode(&key).ok_or_else(|| b"invalid BigMap key".to_vec())?;
                let value =
                    bcs::from_bytes(&value).map_err(|error| error.to_string().into_bytes())?;
                Ok((key, value))
            })
            .collect()
    }
}

fn decode<V: DeserializeOwned>(bytes: Option<Vec<u8>>) -> Result<Option<V>> {
    bytes
        .map(|bytes| bcs::from_bytes(&bytes).map_err(|error| error.to_string().into_bytes()))
        .transpose()
}
