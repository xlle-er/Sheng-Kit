//! Which of two utterances is older (captions.md §6).
//!
//! Service-provided order wins when both utterances carry it; otherwise numeric ids compare
//! numerically and other ids compare by first arrival.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::ids::{BoundedMap, UtteranceId, IDENTITY_LIMIT};

/// Largest service sequence number accepted; larger values are treated as out of range.
pub const MAX_SERVICE_SEQUENCE: i64 = u32::MAX as i64;

#[derive(Debug, Clone)]
pub struct UtteranceOrder {
    arrival: BoundedMap<UtteranceId, u64>,
    next_arrival: u64,
    service: BoundedMap<UtteranceId, u64>,
    owners: HashMap<u64, UtteranceId>,
}

impl Default for UtteranceOrder {
    fn default() -> Self {
        Self::new()
    }
}

impl UtteranceOrder {
    pub fn new() -> Self {
        Self {
            arrival: BoundedMap::new(IDENTITY_LIMIT),
            next_arrival: 0,
            service: BoundedMap::new(IDENTITY_LIMIT),
            owners: HashMap::new(),
        }
    }

    /// Records the first arrival of `id`; later arrivals keep the original rank.
    pub fn observe(&mut self, id: &UtteranceId) {
        if self.arrival.contains_key(id) {
            return;
        }
        self.arrival.insert(id.clone(), self.next_arrival);
        self.next_arrival += 1;
    }

    /// Records service order. Duplicate, reassigned or out-of-range sequences are ignored
    /// and reported as `false`; the state stays usable either way.
    pub fn record_service_order(&mut self, id: &UtteranceId, sequence: i64) -> bool {
        let Ok(sequence) = u64::try_from(sequence) else {
            return false;
        };
        if sequence > MAX_SERVICE_SEQUENCE as u64 {
            return false;
        }
        if let Some(existing) = self.service.get(id) {
            return *existing == sequence;
        }
        if self.owners.contains_key(&sequence) {
            return false;
        }
        self.service.insert(id.clone(), sequence);
        self.owners
            .retain(|_, owner| self.service.contains_key(owner));
        self.owners.insert(sequence, id.clone());
        self.observe(id);
        true
    }

    /// True when `candidate` is known to come before `reference`.
    pub fn is_older(&self, candidate: &UtteranceId, reference: &UtteranceId) -> bool {
        self.compare(candidate, reference) == Some(Ordering::Less)
    }

    pub fn compare(&self, a: &UtteranceId, b: &UtteranceId) -> Option<Ordering> {
        if a == b {
            return Some(Ordering::Equal);
        }
        if let (Some(x), Some(y)) = (self.service.get(a), self.service.get(b)) {
            return Some(x.cmp(y));
        }
        if let (UtteranceId::Number(x), UtteranceId::Number(y)) = (a, b) {
            return Some(x.cmp(y));
        }
        match (self.arrival.get(a), self.arrival.get(b)) {
            (Some(x), Some(y)) => Some(x.cmp(y)),
            _ => None,
        }
    }

    pub fn clear(&mut self) {
        self.arrival.clear();
        self.service.clear();
        self.owners.clear();
    }
}
