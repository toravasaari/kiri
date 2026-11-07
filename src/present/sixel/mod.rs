//! Sixel: the bytes of a picture for terminals that draw pixels written at
//! the cursor. The module is pure: it turns pixels into `DCS ... ST`
//! sequences and makes no system calls.
//!
//! A sixel has no ids and no placements, so kiri draws every image row as a
//! sixel of its own, exactly one cell high (a "slice"), and an image wider
//! than the terminal's sixel limit as tiles side by side, cut at cell
//! boundaries. See `docs/architecture.md`.

mod cache;
mod encode;
mod palette;
mod row;
#[cfg(test)]
mod tests;

pub use cache::Lru;
pub use encode::{Prepared, tiles};
pub use palette::Rgb;
pub use row::{Link, Tile, write_cells};

/// What the terminal lets a sixel use, and what lies under the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// The number of colour registers, at least 2.
    pub registers: usize,
    /// The colour of the cells the image is drawn on, as they show it. The
    /// picture is flattened onto it; without one, pixels that are mostly
    /// transparent stay undrawn and show the cell's own background.
    pub background: Option<Rgb>,
}

impl Options {
    /// 256 registers, which every sixel terminal has, and no background.
    pub const fn new() -> Self {
        Self {
            registers: 256,
            background: None,
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Self::new()
    }
}
