use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct MapKey {
    #[serde(with = "serde_bytes")]
    pub map: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub key: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct MapRange {
    #[serde(with = "serde_bytes")]
    pub map: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub after: Option<Vec<u8>>,
    #[serde(with = "serde_bytes")]
    pub before: Option<Vec<u8>>,
    pub limit: u32,
}
