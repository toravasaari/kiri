//! The diagram languages that kiri draws itself: each has a file here, and
//! one entry in `language.rs`. A program that draws a language is run by
//! the platform, in `src/diagrams/`.

pub(crate) mod error;
pub(crate) mod language;
pub(crate) mod math;
pub(crate) mod mermaid;
