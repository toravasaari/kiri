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
//! the loading that `pipeline.rs` sets going, and the stamps that tell a
//! changed file. The threads that load are in `src/load/queue.rs`.
//!
//! What does not belong here is any new way to read, fetch or draw an
//! image, which is work for threads and programs, and nothing that decodes
//! a format or plans the images, which have files of their own in
//! `src/typeset/images/`.
//!
//! Reading and fetching go to the platform's work runner, as the loads in
//! `src/load/queue.rs` and `src/images/pipeline.rs` do, and decoding and
//! drawing from given data to the typesetting engine.

mod pipeline;
mod stamps;
pub(crate) mod svg;

#[cfg(test)]
pub(crate) use crate::load::queue::{Arrival, Expecting, Gated};
pub use crate::load::queue::{Arrived, Job, Loads, STOPPING};
pub use crate::typeset::images::{
    CodeLook, DEFAULT_MAX_FILE_SIZE, Format, HeadingLook, HeadingWord, ImageError, ImageKey,
    Images, LevelLook, LoadOptions, Looks, MB, Part, Placeholder, Source, decode,
    diagram_languages, diagram_look, draw_heading, heading_words, plain_words, read_image,
};
pub(crate) use crate::typeset::images::{Wanted, plan};
pub use pipeline::{
    Interrupted, Loaded, Loading, jobs, load_documents, load_in_background, planned_keys,
    reload_in_background,
};
pub use stamps::Stamps;
pub use svg::{EMOJI, Fonts, SCRIPTS, Svg, installed};

pub(crate) use crate::typeset::images::decode::{MAX_PIXELS, png_size};
pub(crate) use crate::typeset::images::url::read_at_most;

pub(crate) use crate::typeset::images::error::size;
