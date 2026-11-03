//! How the pager ends: the one place where the loads stop, `close`, as
//! the route calls it once the pager's loop has returned.

use std::time::Duration;

use crate::frontend::sim::close_over_screen;
use crate::images::{ImageKey, Loads};

#[test]
fn close_leaves_the_screen_and_then_waits_for_the_programs_to_end() {
    let ending = Duration::from_millis(400);
    let mut loads = Loads::new();
    loads.program(ImageKey::rendered("mermaid", "slow\n"), ending);
    let closed = close_over_screen(&mut loads);
    // The screen was left at once, before the wait for the program that
    // ends `ending` after it is told to stop.
    assert!(closed.left - closed.started < ending / 2);
    assert!(closed.returned - closed.left >= ending / 2);
    assert!(closed.returned - closed.started >= ending);
    // Nothing is left running: not a load, nor a program.
    assert!(loads.is_empty());
    assert!(loads.wait(Duration::ZERO));
}

#[test]
fn close_with_nothing_loading_returns_at_once() {
    let closed = close_over_screen(&mut Loads::new());
    assert!(closed.returned - closed.started < Duration::from_millis(200));
}
