//! Image code for kiri that debug builds optimise.
//!
//! Generic code is compiled in the crate that uses it, so the generic parts
//! of the image decoders and of fast_image_resize would otherwise be
//! compiled in kiri, without optimisation. This crate uses them through
//! functions that are not generic, and kiri calls those.

mod decode;
mod scale;

pub use decode::{DecodeError, decode_rgba8};
pub use scale::scale_rgba8;
