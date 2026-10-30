//! The typesetting engine: the document model, parsing, themes, the look,
//! both layout engines and the images and diagrams drawn from given data.
//!
//! It is pure given its resources. The fonts, for one, are passed in. It
//! uses none of the other parts of kiri; see docs/architecture.md.

pub mod block;
pub mod caps;
pub mod color;
pub mod compare;
pub mod diagrams;
pub mod images;
pub mod input;
pub mod layout;
pub mod model;
#[cfg(test)]
mod parity;
pub mod parse;
pub mod reading;
pub mod size;
pub mod style;
pub mod theme;
pub mod wrap;
