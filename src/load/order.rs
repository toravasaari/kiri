//! The order of the images that the loads prefer, and taking the next of
//! what waits by it: the loads' queue and the CPU lane both wait in it.
//!
//! `Order` is the order of the images that the loads prefer, which the queue
//! of `runner.rs` and the lane both read, with `rank`; `take_first` is how
//! both take the next of what waits: what was stopped is dropped, and of the
//! rest the first in that order goes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::typeset::images::ImageKey;

/// The order of the images that the loads prefer, by their place in it: one
/// map, which the loads' queue and the lane both read, and which the runner's
/// `prefer` replaces.
#[derive(Debug, Clone, Default)]
pub(crate) struct Order(Arc<Mutex<Arc<HashMap<ImageKey, usize>>>>);

impl Order {
    /// Makes `first` the order, from the next look at it on.
    pub(crate) fn set(&self, first: HashMap<ImageKey, usize>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(first);
    }

    /// The order as it is now, to look at without holding it up.
    pub(crate) fn get(&self) -> Arc<HashMap<ImageKey, usize>> {
        Arc::clone(&self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// How far in the order `first` the image `key` is: its place in it, or
/// last when it has none.
pub(super) fn rank(first: &HashMap<ImageKey, usize>, key: &ImageKey) -> usize {
    first.get(key).copied().unwrap_or(usize::MAX)
}

/// Takes the one of `items` to go next, once those that `stopped` are
/// dropped: of the rest, the one whose `place` is least, which is `None` for
/// one that may not go now. `place` is told the position of the item, and
/// tells the rank in the order and what breaks a tie. `None` when nothing
/// may go, whether none is left or none is allowed.
pub(super) fn take_first<T, K: Ord>(
    items: &mut Vec<T>,
    stopped: impl Fn(&T) -> bool,
    place: impl Fn(usize, &T) -> Option<K>,
) -> Option<T> {
    items.retain(|item| !stopped(item));
    let at = (items.iter().enumerate())
        .filter_map(|(at, item)| place(at, item).map(|place| (place, at)))
        .min_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, at)| at)?;
    Some(items.remove(at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(names: &[&str]) -> HashMap<ImageKey, usize> {
        let place = |(at, name): (usize, &&str)| (ImageKey::from(*name), at);
        names.iter().enumerate().map(place).collect()
    }

    #[test]
    fn an_image_that_is_not_in_the_order_is_last() {
        let first = first(&["a", "b"]);
        assert_eq!(rank(&first, &"b".into()), 1);
        assert_eq!(rank(&first, &"z".into()), usize::MAX);
    }

    #[test]
    fn the_order_is_replaced_for_the_next_look_and_not_for_the_one_held() {
        let order = Order::default();
        order.set(first(&["a"]));
        let held = order.get();
        order.set(first(&["b"]));
        assert_eq!(rank(&held, &"a".into()), 0);
        assert_eq!(rank(&order.get(), &"a".into()), usize::MAX);
    }

    #[test]
    fn the_first_in_the_order_goes_next_and_a_tie_goes_by_the_tie_breaker() {
        let first = first(&["b", "a"]);
        let mut items = vec!["a", "c", "b", "d"];
        let place = |at: usize, item: &&str| Some((rank(&first, &(*item).into()), at));
        let mut taken = Vec::new();
        while let Some(item) = take_first(&mut items, |_| false, place) {
            taken.push(item);
        }
        // `c` and `d` are in no place, and go in the order they waited.
        assert_eq!(taken, ["b", "a", "c", "d"]);
    }

    #[test]
    fn what_was_stopped_is_dropped_without_going() {
        let mut items = vec!["a", "stop", "b"];
        let taken = take_first(&mut items, |item| *item == "stop", |at, _| Some(at));
        assert_eq!(taken, Some("a"));
        assert_eq!(items, ["b"]);
        let mut all = vec!["stop"];
        assert_eq!(
            take_first(&mut all, |_| true, |at, _| Some(at)),
            None::<&str>
        );
        assert!(all.is_empty());
    }

    #[test]
    fn what_may_not_go_now_stays_and_none_is_taken_when_none_may() {
        let mut items = vec!["slow", "fast"];
        let may = |_, item: &&str| (*item != "slow").then_some(0);
        assert_eq!(take_first(&mut items, |_| false, may), Some("fast"));
        assert_eq!(take_first(&mut items, |_| false, may), None);
        assert_eq!(items, ["slow"]);
    }
}
