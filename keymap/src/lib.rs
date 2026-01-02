use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct KeyMap<M: Ord, K: Ord, V> {
    bindings: BTreeMap<M, BTreeMap<K, V>>,
}

impl<M, K, V> Default for KeyMap<M, K, V>
where
    M: Ord,
    K: Ord,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<M, K, V> KeyMap<M, K, V>
where
    M: Ord,
    K: Ord,
{
    pub fn new() -> Self {
        KeyMap {
            bindings: Default::default(),
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

    pub fn iter_mode(&self, mode: &M) -> impl Iterator<Item = (&K, &V)> {
        self.bindings
            .get(mode)
            .into_iter()
            .flat_map(|mode_map| mode_map.iter())
    }

    pub fn merge(&mut self, other: KeyMap<M, K, V>) {
        for (mode, mode_map) in other.bindings {
            let entry = self.bindings.entry(mode).or_default();
            for (key, value) in mode_map {
                entry.insert(key, value);
            }
        }
    }
}

pub trait Action {}
