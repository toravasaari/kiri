//! The images a document refers to, and what is loaded of them.
//!
//! This is the stage between the document model and layout: each image URL
//! is resolved, and each diagram in a code block is drawn as an image, SVG
//! or raster, so layout and output work on pixels and never touch the file
//! system. A failure is a value, not an error for the whole document,
//! because the renderer still shows the alt text in place of an image it
//! could not load.
//!
//! The key of an image, the store of loaded images that layout reads, and
//! the plan of which images a document wants are the typesetting engine's
//! (`src/typeset/images/`), and this file re-exports them as
//! `kiri::images::…`. The functions that read files and run the renderers,
//! `load`, `load_with` and `load_parts`, are in `src/load/`. What is here is
//! the loading that runs on threads, and the stamps that tell a changed
//! file.
//!
//! What does not belong here is any new way to read, fetch or draw an
//! image, which is work for threads and programs, and nothing that decodes
//! a format or plans the images, which have files of their own in
//! `src/typeset/images/`.
//!
//! Reading and fetching go to the platform's work runner, as the loads in
//! `src/images/background.rs` and `src/images/pipeline.rs` do, and
//! decoding and drawing from given data to the typesetting engine.

mod background;
mod pipeline;
mod stamps;
pub(crate) mod svg;

pub use crate::typeset::images::{
    CodeLook, DEFAULT_MAX_FILE_SIZE, Format, HeadingLook, HeadingWord, ImageError, ImageKey,
    Images, LevelLook, LoadOptions, MB, Part, Placeholder, Source, decode, diagram_languages,
    draw_heading, heading_words, plain_words, read_image,
};
pub(crate) use crate::typeset::images::{Wanted, plan};
#[cfg(test)]
pub(crate) use background::{Arrival, Gated};
pub use background::{Loads, STOPPING};
pub use pipeline::{
    Interrupted, Loaded, Loading, load_documents, load_headings_in_background, load_in_background,
    load_inline_in_background, measure_in_background, reload_in_background,
};
pub use stamps::Stamps;
pub use svg::{EMOJI, Fonts, SCRIPTS, Svg, installed};

pub(crate) use crate::typeset::images::decode::{MAX_PIXELS, png_size};
#[cfg(all(test, feature = "mermaid"))]
pub(crate) use crate::typeset::images::drawn::decode_diagram;
pub(crate) use crate::typeset::images::url::read_at_most;

pub(crate) use crate::typeset::images::error::size;
