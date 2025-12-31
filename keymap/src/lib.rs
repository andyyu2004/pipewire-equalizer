use std::{collections::HashMap, hash::Hash};

#[derive(Debug, Default)]
pub struct KeyMap<K, V> {
    map: HashMap<K, V>,
}

impl<K, V> KeyMap<K, V>
where
    K: Hash + Eq,
    V: Copy,
{
    pub fn new() -> Self {
        KeyMap {
            map: HashMap::new(),
        }
    }

    pub fn bind(&mut self, key: K, value: V) {
        self.map.insert(key, value);
    }

    pub fn get(&self, key: &K) -> Option<V> {
        self.map.get(key).copied()
    }
}

pub trait Action {}
