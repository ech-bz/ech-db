use super::{decode, BigMap};
use crate::{Host, Key, Result};
use ech_db_protocol::map::MapKey;
use serde::de::DeserializeOwned;

mod private {
    pub trait Sealed {}
}

pub trait BatchRead: private::Sealed {
    type Key;
    type Value;
    type Output;

    #[doc(hidden)]
    fn keys(&self, keys: &mut Vec<MapKey>) -> Result<()>;

    #[doc(hidden)]
    fn decode(&self, values: &mut std::vec::IntoIter<Option<Vec<u8>>>) -> Result<Self::Output>;
}

pub(super) fn get_batch<B: BatchRead>(batch: B) -> Result<B::Output> {
    let mut keys = Vec::new();
    batch.keys(&mut keys)?;
    let values = if keys.is_empty() {
        Vec::new()
    } else {
        Host::map_get(&keys)
    };
    if values.len() != keys.len() {
        return Err(b"invalid BigMap batch response length".to_vec());
    }
    batch.decode(&mut values.into_iter())
}

impl<K, V> private::Sealed for (&BigMap<K, V>, &K) {}

impl<K: Key, V: DeserializeOwned> BatchRead for (&BigMap<K, V>, &K) {
    type Key = K;
    type Value = V;
    type Output = Option<V>;

    fn keys(&self, keys: &mut Vec<MapKey>) -> Result<()> {
        keys.push(self.0.key(self.1)?);
        Ok(())
    }

    fn decode(&self, values: &mut std::vec::IntoIter<Option<Vec<u8>>>) -> Result<Self::Output> {
        decode(
            values
                .next()
                .ok_or_else(|| b"missing BigMap batch result".to_vec())?,
        )
    }
}

impl<K, V> private::Sealed for (&BigMap<K, V>, &[K]) {}

impl<K: Key, V: DeserializeOwned> BatchRead for (&BigMap<K, V>, &[K]) {
    type Key = K;
    type Value = V;
    type Output = Vec<Option<V>>;

    fn keys(&self, keys: &mut Vec<MapKey>) -> Result<()> {
        for key in self.1 {
            (self.0, key).keys(keys)?;
        }
        Ok(())
    }

    fn decode(&self, values: &mut std::vec::IntoIter<Option<Vec<u8>>>) -> Result<Self::Output> {
        self.1
            .iter()
            .map(|key| (self.0, key).decode(values))
            .collect()
    }
}

impl<B: BatchRead + ?Sized> private::Sealed for &B {}

impl<B: BatchRead + ?Sized> BatchRead for &B {
    type Key = B::Key;
    type Value = B::Value;
    type Output = B::Output;

    fn keys(&self, keys: &mut Vec<MapKey>) -> Result<()> {
        B::keys(self, keys)
    }

    fn decode(&self, values: &mut std::vec::IntoIter<Option<Vec<u8>>>) -> Result<Self::Output> {
        B::decode(self, values)
    }
}

impl<K, V> private::Sealed for [(&BigMap<K, V>, &K)] {}

impl<K: Key, V: DeserializeOwned> BatchRead for [(&BigMap<K, V>, &K)] {
    type Key = K;
    type Value = V;
    type Output = Vec<Option<V>>;

    fn keys(&self, keys: &mut Vec<MapKey>) -> Result<()> {
        for (map, key) in self {
            (*map, *key).keys(keys)?;
        }
        Ok(())
    }

    fn decode(&self, values: &mut std::vec::IntoIter<Option<Vec<u8>>>) -> Result<Self::Output> {
        self.iter()
            .map(|(map, key)| (*map, *key).decode(values))
            .collect()
    }
}

macro_rules! tuple_batch {
    ($first:ident : $first_index:tt $(, $ty:ident : $index:tt)*) => {
        impl<$first: BatchRead, $($ty: BatchRead),*> private::Sealed for ($first, $($ty,)*) {}

        impl<$first: BatchRead, $($ty: BatchRead),*> BatchRead for ($first, $($ty,)*) {
            type Key = $first::Key;
            type Value = $first::Value;
            type Output = ($first::Output, $($ty::Output,)*);

            fn keys(&self, keys: &mut Vec<MapKey>) -> Result<()> {
                self.$first_index.keys(keys)?;
                $(self.$index.keys(keys)?;)*
                Ok(())
            }

            fn decode(&self, values: &mut std::vec::IntoIter<Option<Vec<u8>>>) -> Result<Self::Output> {
                Ok((self.$first_index.decode(values)?, $(self.$index.decode(values)?,)*))
            }
        }
    };
}

tuple_batch!(A: 0);
tuple_batch!(A: 0, B: 1);
tuple_batch!(A: 0, B: 1, C: 2);
tuple_batch!(A: 0, B: 1, C: 2, D: 3);
tuple_batch!(A: 0, B: 1, C: 2, D: 3, E: 4);
tuple_batch!(A: 0, B: 1, C: 2, D: 3, E: 4, F: 5);
tuple_batch!(A: 0, B: 1, C: 2, D: 3, E: 4, F: 5, G: 6);
tuple_batch!(A: 0, B: 1, C: 2, D: 3, E: 4, F: 5, G: 6, H: 7);
