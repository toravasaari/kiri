//! kiri renders Markdown documents for the terminal.

pub mod caps;
pub mod color;
pub mod compare;
pub mod config;
pub mod diagrams;
pub mod doctor;
pub mod images;
pub mod input;
pub mod kitty;
pub mod list;
pub mod model;
pub mod pager;
pub mod panics;
pub mod parse;
pub mod render;
pub mod size;
pub mod term;
pub mod theme;

#[cfg(test)]
mod testing;
