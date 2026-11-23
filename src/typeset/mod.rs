//! The typesetting engine: the document model, parsing, themes, the look,
//! both layout engines and the images and diagrams drawn from given data.
//!
//! It is pure given its resources. The fonts, for one, are passed in. It
//! uses none of the other parts of kiri; see docs/architecture.md.

pub mod align;
pub mod anchors;
pub mod block;
pub mod caps;
pub mod color;
pub mod compare;
pub mod contents;
pub mod diagnostic;
pub mod diagrams;
pub mod driver;
pub mod facts;
pub mod fonts;
pub mod grid;
pub mod images;
pub mod input;
pub mod laid;
pub mod layers;
pub mod layout;
pub mod loading;
pub mod locate;
#[cfg(test)]
mod measured;
pub mod model;
#[cfg(test)]
mod parity;
pub mod parse;
pub mod position;
pub mod reading;
pub(crate) mod rendition;
pub mod size;
pub mod source_rows;
pub mod stretch;
pub mod style;
pub mod targets;
pub mod text;
pub mod theme;
pub(crate) mod walk;
pub mod wrap;
