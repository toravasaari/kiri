//! The front ends' tests: the route run in the simulated front end
//! (`src/frontend/sim.rs`), through the same `Platform` the binary
//! implements for the process. One file for each topic: `route.rs` (one
//! file, several, standard input, a missing file), `paging.rs` (paging
//! and printing), `settings.rs`, `stdin.rs`, `follow.rs`, `reload.rs`, and
//! `close.rs` (how the pager ends).

mod close;
mod follow;
mod paging;
mod reload;
mod route;
mod settings;
mod stdin;

/// `count` paragraphs of one row each, `Line 1` to `Line count`.
fn paragraphs(count: usize) -> String {
    (1..=count).map(|n| format!("Line {n}\n\n")).collect()
}

/// A document longer than a window of 5 rows: `Line 1` to `Line 10`, one
/// row each and a blank row between.
fn long() -> String {
    paragraphs(10)
}

/// The text of `long.md` written anew: ten paragraphs of `Fresh`.
fn fresh() -> String {
    (1..=10).map(|n| format!("Fresh {n}\n\n")).collect()
}
