//! Session-local numeric keys for complete event and occurrence identities.

use enzui_core::civil_date::Date;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default)]
pub(crate) struct OccurrenceIds {
    by_pair: HashMap<(u64, Date), u64>,
    pairs: HashMap<u64, (u64, Date)>,
    next: u64,
}

impl OccurrenceIds {
    pub(crate) fn key_for(&mut self, event_id: u64, date: Date) -> Option<u64> {
        let pair = (event_id, date);
        if let Some(key) = self.by_pair.get(&pair) {
            return Some(*key);
        }
        let key = self.next;
        let next = key.checked_add(1)?;
        self.by_pair.try_reserve(1).ok()?;
        self.pairs.try_reserve(1).ok()?;
        self.pairs.insert(key, pair);
        self.by_pair.insert(pair, key);
        self.next = next;
        Some(key)
    }

    pub(crate) fn pair_for(&self, key: u64) -> Option<(u64, Date)> {
        self.pairs.get(&key).copied()
    }

    pub(crate) fn retain_keys(&mut self, keys: impl IntoIterator<Item = u64>) {
        let live: HashSet<_> = keys.into_iter().collect();
        self.pairs.retain(|key, _| live.contains(key));
        self.by_pair.retain(|_, key| live.contains(key));
        // Retired keys are never reused: delayed actions must not target another occurrence.
        let capacity = live.len().saturating_mul(4).max(64);
        if self.pairs.capacity() > capacity {
            self.pairs.shrink_to(capacity);
        }
        if self.by_pair.capacity() > capacity {
            self.by_pair.shrink_to(capacity);
        }
    }
}
