//! A least-recently-used cache with a budget in bytes, for the slices kiri
//! has encoded and the pictures it prepared them from.

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

/// Values held by key, each with a weight in bytes; the budget is the sum of
/// the weights. When an insert goes over it, the values used longest ago go.
pub struct Lru<K, V> {
    budget: usize,
    weight: usize,
    tick: u64,
    entries: HashMap<K, Entry<V>>,
}

struct Entry<V> {
    value: Rc<V>,
    weight: usize,
    used: u64,
}

impl<K: Hash + Eq + Clone, V> Lru<K, V> {
    /// An empty cache that holds at most `budget` bytes, one value over it
    /// at most: a value heavier than the budget is still kept, alone.
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            weight: 0,
            tick: 0,
            entries: HashMap::new(),
        }
    }

    /// The value held for `key`, which counts as used now.
    pub fn get(&mut self, key: &K) -> Option<Rc<V>> {
        self.tick += 1;
        let entry = self.entries.get_mut(key)?;
        entry.used = self.tick;
        Some(Rc::clone(&entry.value))
    }

    /// Holds `value`, of `weight` bytes, for `key`, and lets go of the values
    /// used longest ago while the cache is over its budget.
    pub fn put(&mut self, key: K, value: V, weight: usize) -> Rc<V> {
        self.tick += 1;
        let value = Rc::new(value);
        let entry = Entry {
            value: Rc::clone(&value),
            weight,
            used: self.tick,
        };
        if let Some(old) = self.entries.insert(key, entry) {
            self.weight -= old.weight;
        }
        self.weight += weight;
        if self.weight > self.budget {
            self.shrink();
        }
        value
    }

    /// Lets go of the oldest values until three quarters of the budget are
    /// left, so that the next puts do not each have to.
    fn shrink(&mut self) {
        let mut by_age: Vec<(u64, K)> = (self.entries.iter())
            .map(|(key, entry)| (entry.used, key.clone()))
            .collect();
        by_age.sort_by_key(|(used, _)| *used);
        let target = self.budget / 4 * 3;
        // The newest value stays, though it alone may be over the budget.
        for (_, key) in by_age.iter().take(by_age.len().saturating_sub(1)) {
            if self.weight <= target {
                break;
            }
            if let Some(entry) = self.entries.remove(key) {
                self.weight -= entry.weight;
            }
        }
    }

    /// How many values are held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is held.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The bytes held.
    pub fn weight(&self) -> usize {
        self.weight
    }
}

impl<K, V> Default for Lru<K, V> {
    fn default() -> Self {
        Self {
            budget: 0,
            weight: 0,
            tick: 0,
            entries: HashMap::new(),
        }
    }
}

impl<K, V> fmt::Debug for Lru<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lru")
            .field("values", &self.entries.len())
            .field("bytes", &self.weight)
            .field("budget", &self.budget)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_values_used_longest_ago_go_first_when_the_budget_is_passed() {
        let mut cache: Lru<u32, &str> = Lru::new(100);
        cache.put(1, "a", 30);
        cache.put(2, "b", 30);
        cache.put(3, "c", 30);
        // Using 1 makes 2 the oldest, then 3.
        assert!(cache.get(&1).is_some());
        cache.put(4, "d", 30);
        assert!(cache.get(&2).is_none() && cache.get(&3).is_none());
        assert!(cache.get(&1).is_some() && cache.get(&4).is_some());
        assert!(cache.weight() <= 100);
    }

    #[test]
    fn a_value_heavier_than_the_budget_is_kept_alone() {
        let mut cache: Lru<u32, &str> = Lru::new(10);
        cache.put(1, "a", 4);
        cache.put(2, "b", 50);
        assert_eq!(cache.len(), 1);
        assert!(cache.get(&2).is_some());
    }

    #[test]
    fn putting_under_a_key_again_replaces_its_weight() {
        let mut cache: Lru<u32, &str> = Lru::new(100);
        cache.put(1, "a", 40);
        cache.put(1, "b", 10);
        assert_eq!((cache.len(), cache.weight()), (1, 10));
        assert_eq!(*cache.get(&1).unwrap(), "b");
    }
}
