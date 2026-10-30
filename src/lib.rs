//! kiri renders Markdown documents for the terminal.

pub mod config;
pub mod diagrams;
pub mod doctor;
pub mod images;
pub mod list;
pub mod pager;
pub mod panics;
pub mod present;
pub mod render;
pub mod term;
pub mod typeset;

#[cfg(test)]
mod testing;
