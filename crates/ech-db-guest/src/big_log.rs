use crate::{BigMap, Bind, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Bind, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct BigLog<T> {
    entries: BigMap<u64, T>,
    length: u64,
}

impl<T> BigLog<T> {
    pub fn new() -> Self {
        Self {
            entries: BigMap::new(),
            length: 0,
        }
    }

    pub fn len(&self) -> u64 {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}
impl<T> Default for BigLog<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> Clone for BigLog<T> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            length: self.length,
        }
    }
}
impl<T> PartialEq for BigLog<T> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries && self.length == other.length
    }
}
impl<T> Eq for BigLog<T> {}
impl<T> std::fmt::Debug for BigLog<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BigLog")
    }
}
impl<T: Serialize> BigLog<T> {
    pub fn push(&mut self, value: &T) -> Result<u64> {
        let index = self.length;
        let next = index
            .checked_add(1)
            .ok_or_else(|| b"BigLog length overflow".to_vec())?;
        self.entries.insert(&index, value)?;
        self.length = next;
        Ok(index)
    }
}
impl<T: DeserializeOwned> BigLog<T> {
    pub fn get(&self, index: u64) -> Result<Option<T>> {
        self.entries.get(&index)
    }
    pub fn get_many(&self, indices: &[u64]) -> Result<Vec<Option<T>>> {
        self.entries.get_many(indices)
    }
    pub fn entries(&self, after: Option<u64>, limit: usize) -> Result<Vec<(u64, T)>> {
        self.entries.entries(after.as_ref(), limit)
    }
    pub fn tail(&self, before: Option<u64>, limit: usize) -> Result<Vec<(u64, T)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let length = self.length;
        let end = before.unwrap_or(length).min(length);
        let start = end.saturating_sub(limit as u64);
        let mut rows = self
            .entries
            .range(start.checked_sub(1).as_ref(), Some(&end), limit)?;
        rows.reverse();
        Ok(rows)
    }
}
