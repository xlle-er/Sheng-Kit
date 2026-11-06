//! Identities: audio sources, utterances, caption ids and bounded identity sets.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

/// Maximum number of active identities kept per source for dedupe, pairing and clear records.
pub const IDENTITY_LIMIT: usize = 64;
/// Fingerprints kept per source for confirmed ids that were evicted from the active set.
pub const RETIRED_IDENTITY_LIMIT: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    System,
    Microphone,
}

/// Utterance identity given by a service, either a string or a number.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UtteranceId {
    Number(u64),
    Text(String),
}

impl From<u64> for UtteranceId {
    fn from(value: u64) -> Self {
        Self::Number(value)
    }
}

impl From<&str> for UtteranceId {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

/// Stable 64-bit fingerprint of an utterance id; stores no text beyond the hash.
pub fn fingerprint(id: &UtteranceId) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

/// Produces `<runId>:<generation>:<seq>` caption ids; `seq` never repeats within one handle.
#[derive(Debug, Clone)]
pub struct CaptionIdFactory {
    run_id: String,
    generation: u64,
    next_seq: u64,
}

impl CaptionIdFactory {
    pub fn new(run_id: impl Into<String>, generation: u64) -> Self {
        Self {
            run_id: run_id.into(),
            generation,
            next_seq: 0,
        }
    }

    /// Switches to a new generation while keeping `seq` monotonic.
    pub fn set_generation(&mut self, generation: u64) {
        self.generation = generation;
    }

    pub fn next_id(&mut self) -> String {
        let id = format!("{}:{}:{}", self.run_id, self.generation, self.next_seq);
        self.next_seq += 1;
        id
    }
}

/// Set with first-in-first-out eviction once `capacity` members are present.
#[derive(Debug, Clone)]
pub struct BoundedSet<T> {
    order: VecDeque<T>,
    members: HashSet<T>,
    capacity: usize,
}

impl<T: Clone + Eq + Hash> BoundedSet<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            order: VecDeque::new(),
            members: HashSet::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn contains(&self, value: &T) -> bool {
        self.members.contains(value)
    }

    /// Inserts `value`; returns the evicted member, if any.
    pub fn insert(&mut self, value: T) -> Option<T> {
        if !self.members.insert(value.clone()) {
            return None;
        }
        self.order.push_back(value);
        if self.order.len() <= self.capacity {
            return None;
        }
        let evicted = self.order.pop_front()?;
        self.members.remove(&evicted);
        Some(evicted)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.members.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.order.iter()
    }
}

/// Map with first-in-first-out eviction once `capacity` keys are present.
#[derive(Debug, Clone)]
pub struct BoundedMap<K, V> {
    order: VecDeque<K>,
    entries: HashMap<K, V>,
    capacity: usize,
}

impl<K: Clone + Eq + Hash, V> BoundedMap<K, V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            order: VecDeque::new(),
            entries: HashMap::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Inserts or replaces `key`; a replaced key keeps its eviction position.
    pub fn insert(&mut self, key: K, value: V) {
        if self.entries.insert(key.clone(), value).is_some() {
            return;
        }
        self.order.push_back(key);
        if self.order.len() > self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(&evicted);
            }
        }
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let value = self.entries.remove(key)?;
        self.order.retain(|existing| existing != key);
        Some(value)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.values()
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.order.iter()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.entries.clear();
    }
}
