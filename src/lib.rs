//! kiri renders Markdown documents for the terminal.

pub mod config;
pub mod diagrams;
pub mod doctor;
pub mod frontend;
pub mod images;
pub mod list;
pub mod load;
pub mod pager;
pub mod panics;
pub mod present;
pub mod render;
pub mod session;
pub mod term;
pub mod typeset;

#[cfg(test)]
mod testing;

// The check of a layout made in steps is shared with the integration tests,
// which name the crate `kiri`.
#[cfg(test)]
extern crate self as kiri;
#[cfg(test)]
#[path = "../tests/it/common/steps.rs"]
#[allow(dead_code)]
pub(crate) mod steps;
