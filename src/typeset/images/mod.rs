//! The pure parts of loading images: decoding a format, drawing a heading
//! or a diagram from data, and the size of an image.
//!
//! The functions that read files and run the renderers stay in
//! `src/images/`, the platform's part.

pub(crate) mod decode;
pub(crate) mod drawn;
pub(crate) mod error;
pub(crate) mod heading;
pub(crate) mod key;
pub mod size;
pub(crate) mod url;

pub use error::ImageError;
pub use key::ImageKey;
