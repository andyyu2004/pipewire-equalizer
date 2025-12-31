use std::{collections::HashMap, hash::Hash};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct KeyMap<M: Eq + Hash, K: Eq + Hash, V> {
    bindings: HashMap<M, HashMap<K, V>>,
}

impl<M, K, V> Default for KeyMap<M, K, V>
where
    M: Hash + Eq,
    K: Hash + Eq,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<M, K, V> KeyMap<M, K, V>
where
    M: Hash + Eq,
    K: Hash + Eq,
{
    pub fn new() -> Self {
        KeyMap {
            bindings: HashMap::new(),
        }
    }

    pub fn bind(&mut self, mode: M, key: K, value: V) -> Option<V> {
        self.bindings.entry(mode).or_default().insert(key, value)
    }

    pub fn get(&self, mode: &M, key: &K) -> Option<&V> {
        self.bindings
            .get(mode)
            .and_then(|mode_map| mode_map.get(key))
    }
}

pub trait Action {}
