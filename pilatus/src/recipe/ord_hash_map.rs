use std::collections::HashMap;
use std::fmt::{self, Formatter};
use std::hash::Hash;
use std::marker::PhantomData;

use serde::de::Visitor;
use serde::{Deserialize, Serialize};

// Similar to a Btree, but stores itself as an array (JSON doesn't guarantee Object ordering)
// [['key1', 'value1'], ['key2', 'value2'], ...]
#[derive(Debug, Clone)]
pub struct OrdHashMap<K, V>(HashMap<K, (usize, V)>);

impl<K, V> Default for OrdHashMap<K, V> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<K: Eq + Hash, V> OrdHashMap<K, V> {
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.0.values_mut().map(|(_, x)| x)
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.0.values().map(|(_, x)| x)
    }
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.0.insert(key, (self.0.len(), value)).map(|(_, x)| x)
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.get(key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.0.get_mut(key).map(|(_, v)| v)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&'_ K, &'_ V)> {
        self.0.iter().map(|(k, (_, v))| (k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.0.keys()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        if let Some((rcnt, rval)) = self.0.remove(key) {
            self.0.iter_mut().for_each(|(_, (cnt, _))| {
                if *cnt > rcnt {
                    *cnt -= 1
                }
            });
            Some(rval)
        } else {
            None
        }
    }
}

impl<const SIZE: usize, K: Eq + Hash, V> From<[(K, V); SIZE]> for OrdHashMap<K, V> {
    fn from(input: [(K, V); SIZE]) -> Self {
        OrdHashMap(
            input
                .into_iter()
                .enumerate()
                .map(|(x, (k, v))| (k, (x, v)))
                .collect(),
        )
    }
}

impl<K: Serialize, V: Serialize> Serialize for OrdHashMap<K, V> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut x = self.0.iter().collect::<Vec<_>>();
        x.sort_by_key(|(_, (v, _))| v);
        serializer.collect_map(x.into_iter().map(|(k, (_, v))| (k, v)))
    }
}

impl<'de, K: Deserialize<'de> + Hash + Eq, V: Deserialize<'de>> Deserialize<'de>
    for OrdHashMap<K, V>
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(SeqVisitor::<K, V>(PhantomData))
    }
}

struct SeqVisitor<K, V>(PhantomData<(K, V)>);

impl<'de, K: Deserialize<'de> + Hash + Eq, V: Deserialize<'de>> Visitor<'de> for SeqVisitor<K, V> {
    type Value = OrdHashMap<K, V>;

    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "a sequence of 2 items")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut all = OrdHashMap(Default::default());
        let mut ctr = 0;
        while let Some((k, v)) = map.next_entry::<K, V>()? {
            all.0.insert(k, (ctr, v));
            ctr += 1;
        }

        Ok(all)
    }
}
