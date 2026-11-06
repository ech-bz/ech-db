use crate::{BigMap, Bind, Key, Result};
use serde::{Deserialize, Serialize};

#[derive(Bind, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct BigSet<K> {
    rows: BigMap<K, ()>,
}

impl<K> BigSet<K> {
    pub fn new() -> Self {
        Self {
            rows: BigMap::new(),
        }
    }
}
impl<K> Default for BigSet<K> {
    fn default() -> Self {
        Self::new()
    }
}
impl<K> Clone for BigSet<K> {
    fn clone(&self) -> Self {
        Self {
            rows: self.rows.clone(),
        }
    }
}
impl<K> PartialEq for BigSet<K> {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
    }
}
impl<K> Eq for BigSet<K> {}
impl<K> std::fmt::Debug for BigSet<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BigSet")
    }
}
impl<K: Key> BigSet<K> {
    pub fn contains(&self, key: &K) -> Result<bool> {
        Ok(self.rows.get(key)?.is_some())
    }
    pub fn contains_many(&self, keys: &[K]) -> Result<Vec<bool>> {
        Ok(self
            .rows
            .get_many(keys)?
            .into_iter()
            .map(|v| v.is_some())
            .collect())
    }
    pub fn insert(&self, key: &K) -> Result<()> {
        self.rows.insert(key, &())
    }
    pub fn remove(&self, key: &K) -> Result<()> {
        self.rows.remove(key)
    }
    pub fn entries(&self, after: Option<&K>, limit: usize) -> Result<Vec<K>> {
        self.range(after, None, limit)
    }
    pub fn range(&self, from: Option<&K>, to: Option<&K>, limit: usize) -> Result<Vec<K>> {
        Ok(self
            .rows
            .range(from, to, limit)?
            .into_iter()
            .map(|(key, ())| key)
            .collect())
    }
}
