//! What an external command writes, which its configuration file's text
//! depends on.

/// What a command writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// SVG, or a raster image at one pixel to the CSS pixel.
    Svg,
    /// A raster image, at the scale that `src/diagrams/external.rs` draws
    /// at, when the command takes `{scale}`.
    Raster,
}
