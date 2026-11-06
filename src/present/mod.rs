//! The terminal layer: the bytes kiri sends to a terminal and the decoding
//! of what the terminal sends back. It is pure and sans-IO: it writes to an
//! `impl Write` and reads the bytes it is given, and makes no system calls.
//! Kitty's own sequences live in `kitty/`.
//!
//! The I/O that reaches the terminal is in `src/term/`, the platform's part.

pub mod cell_query;
pub mod clipboard;
pub mod encoder;
pub mod keys;
pub mod kitty;
pub(crate) mod ledger;
pub mod output;
pub mod presenter;
pub mod scene;

pub(crate) mod answers;
