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
