//! The pure parts of loading images: decoding a format, drawing a heading
//! or a diagram from data, and the size of an image.
//!
//! The functions that read files and run the renderers stay in
//! `src/images/`, the platform's part.

pub(crate) mod decode;
pub(crate) mod drawn;
pub(crate) mod heading;
pub mod size;
