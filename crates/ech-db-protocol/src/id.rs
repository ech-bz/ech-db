use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::HexError;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Root([u8; 32]);

impl Root {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_array(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl FromStr for Root {
    type Err = HexError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(text).map_err(|_| HexError)?;
        let array: [u8; 32] = bytes.try_into().map_err(|_| HexError)?;
        Ok(Self(array))
    }
}

impl fmt::Display for Root {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Txid([u8; 32]);

impl Txid {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        let array: [u8; 32] = bytes.try_into().ok()?;
        Some(Self(array))
    }

    pub fn as_array(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl FromStr for Txid {
    type Err = HexError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(text).map_err(|_| HexError)?;
        let array: [u8; 32] = bytes.try_into().map_err(|_| HexError)?;
        Ok(Self(array))
    }
}

impl fmt::Display for Txid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}
