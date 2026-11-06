//! The pure parts of loading images: decoding a format, drawing a heading
//! or a diagram from data, the size of an image, and the plan of which
//! images a document wants, with the store of what was loaded.
//!
//! The functions that read files and run the renderers are in `src/load/`,
//! the platform's part; the loads that run on threads are in
//! `src/load/queue.rs`.

pub mod animation;
pub mod build;
pub(crate) mod decode;
pub(crate) mod drawn;
pub(crate) mod error;
pub(crate) mod heading;
pub(crate) mod key;
pub mod marks;
pub(crate) mod options;
pub(crate) mod plan;
pub mod size;
pub(crate) mod store;
pub(crate) mod url;
pub(crate) mod warnings;

pub use decode::{
    DEFAULT_MAX_FILE_SIZE, Format, MB, Source, decode, read_image, read_image_as, read_size_as,
};
pub use error::ImageError;
pub use heading::{
    CodeLook, HeadingLook, HeadingWord, LevelLook, draw_heading, heading_words, plain_words,
};
pub use key::ImageKey;
pub use options::{LoadOptions, Looks, Part, diagram_look, diagram_look_for, formula_colors};
pub use plan::diagram_languages;
pub(crate) use plan::{Wanted, plan};
pub use store::{Images, Placeholder};
