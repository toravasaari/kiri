//! The diagram languages that kiri draws itself: each has a file here, and
//! one entry in `language.rs`. A program that draws a language is run by
//! the platform, in `src/diagrams/` and `src/load/`.
//!
//! What a renderer is spoken to in is `request.rs`, the `Renderer` trait and
//! the renderers built in are `renderer.rs`, and what a message says of a
//! language by its name is `names.rs`.

pub(crate) mod error;
pub(crate) mod language;
pub(crate) mod math;
pub(crate) mod mermaid;
pub(crate) mod names;
pub(crate) mod output;
pub(crate) mod renderer;
pub(crate) mod request;
