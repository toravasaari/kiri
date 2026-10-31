//! Diagrams written as code, such as Mermaid, drawn as images.
//!
//! A renderer takes a diagram's source and a theme and gives an image
//! file: SVG text, or a raster image such as the PNG of a browser's
//! drawing. It knows nothing about terminals: the image loader decodes it
//! as it does an image file, and another backend could use the same
//! image.
//!
//! The types a diagram renderer is spoken to in, a request, a drawing and
//! the `Renderer` trait, the renderers built in and the names of the
//! languages are the typesetting engine's (`src/typeset/diagrams/`), and
//! this file re-exports them as `kiri::diagrams::…`. What is here is the
//! platform's: `external.rs` runs a program that draws, and `find.rs`
//! finds one. `Renderers`, which chooses between them and the built-in
//! renderer, and the threads that draw several diagrams at once are in
//! `src/load/`.
//!
//! A diagram language does not belong here, nor a new renderer built into
//! kiri: they are files of their own in `src/typeset/diagrams/`, with one
//! entry each in `language.rs`. A new way to find or run a program does
//! belong to the platform. The placement map in docs/architecture.md has
//! the paths.

pub mod external;
pub mod find;

pub use crate::typeset::diagrams::error::DiagramError;
pub use crate::typeset::diagrams::language::{LANGUAGES, Language, ThemeNames, language};
pub use crate::typeset::diagrams::mermaid::MMDC;
pub use crate::typeset::diagrams::names::{
    centred, chosen_theme, dark_background, display_name, extension, name, noun, theme_name,
};
pub use crate::typeset::diagrams::renderer::{BuiltIn, NoRenderer, Renderer, baseline_depth};
pub use crate::typeset::diagrams::request::{Drawn, FormulaColors, INLINE_FORMULA, Request, Theme};
pub use external::{Command, External, ExternalSettings};
