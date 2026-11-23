//! `Output`: what an external command writes, which a language's configuration
//! file's text depends on.
//!
//! The program itself is run by the platform, in `src/diagrams/external.rs`.

/// What a command writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// SVG, or a raster image at one pixel to the CSS pixel.
    Svg,
    /// A raster image, at the scale that `src/diagrams/external.rs` draws
    /// at, when the command takes `{scale}`.
    Raster,
}
