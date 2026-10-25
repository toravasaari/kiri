//! Panics that kiri catches on purpose, told apart from the others: a
//! load in the background catches a renderer's or a decoder's panic and
//! fails, and the pager's screen stays open for it, while any other panic
//! leaves the screen before its message is written.

use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};

thread_local! {
    /// Whether this thread runs [`catch`] now.
    static CATCHING: Cell<bool> = const { Cell::new(false) };
}

/// Runs `work`, catching a panic in it, which [`catching`] tells while
/// it unwinds.
pub fn catch<T>(work: impl FnOnce() -> T) -> std::thread::Result<T> {
    let before = CATCHING.replace(true);
    let caught = panic::catch_unwind(AssertUnwindSafe(work));
    CATCHING.set(before);
    caught
}

/// Whether a panic on this thread now is one that [`catch`] catches.
pub fn catching() -> bool {
    CATCHING.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catching_is_told_only_inside_catch() {
        assert!(!catching());
        let inside = catch(catching);
        assert_eq!(inside.ok(), Some(true));
        assert!(!catching());
        let caught = catch(|| -> () { panic!("caught") });
        assert!(caught.is_err());
        assert!(!catching());
    }
}
