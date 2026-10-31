//! Settings from the command line, the environment and the configuration
//! file, read once at start-up. The environment also holds the variables
//! that capability detection reads.
//!
//! Everything here takes the environment as a value, so the rules can be
//! tested without touching the process environment.
//!
//! The module is in five files: `registry.rs` (each setting once, and the
//! types and the rule it generates), `environment.rs` (the variables and
//! the directories), `file.rs` (the configuration file), `resolve.rs`
//! (which value a setting gets) and `types.rs` (the setting types). The
//! types the pager reads are in `src/session/settings.rs`.
//!
//! What a setting does does not belong here: no code that uses the value,
//! no I/O beyond reading the file and the environment, and no type that
//! only one feature reads. `Passthrough` stays a setting type here,
//! though capability detection, the command line and `--doctor` read it;
//! capability detection takes its value, and the variables it reads
//! (`TMUX`, `TMUX_PANE`, `NO_COLOR`, `COLORTERM`, `TERM_PROGRAM` and
//! `VTE_VERSION`), as arguments, not `Environment`. The placement map in
//! docs/architecture.md says where a setting goes.

mod environment;
mod file;
mod registry;
mod resolve;
mod types;

pub use environment::*;
pub use file::*;
pub use registry::*;
pub use resolve::*;
pub use types::*;

#[cfg(test)]
mod tests;
