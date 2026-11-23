//! Drawing the parts of a reading-mode page: the pixels of a part, and of a
//! slice of a band, through the glyphs every part shares.
//!
//! `Parts::draw` and `Parts::draw_band` are drawn through the glyph cache of
//! the `Reading`. They are in their own file, apart from `engine.rs`.

use image::RgbaImage;

use crate::typeset::reading::engine::{PART_ROWS, Parts};
use crate::typeset::reading::page::Overlay;

impl Parts {
    /// The pixels of part `part`.
    pub fn draw(&self, part: u32, overlay: Option<&Overlay>) -> RgbaImage {
        let (width, height) = (u32::from(self.cell.0), u32::from(self.cell.1));
        let first = part * u32::from(PART_ROWS);
        let rows = (u32::from(self.rows) - first).min(u32::from(PART_ROWS));
        let mut glyphs = self.glyphs.borrow_mut();
        (self.page).draw_with(
            &mut glyphs,
            first * height,
            u32::from(self.columns_of(part)) * width,
            rows * height,
            overlay,
        )
    }

    /// The pixels of the rows of slice `slice` of band `band`, scrolled
    /// `offset` pixels, `selected` or not, as wide as the page.
    pub fn draw_band(
        &self,
        band: u32,
        slice: u32,
        offset: u32,
        selected: bool,
        overlay: Option<&Overlay>,
    ) -> RgbaImage {
        let cell = f32::from(self.cell.1);
        let total = (self.page.bands.get(band as usize)).map_or(0.0, |band| band.height);
        let from = (slice * u32::from(PART_ROWS)) as f32 * cell;
        let to = (from + f32::from(PART_ROWS) * cell).min(total);
        let mut glyphs = self.glyphs.borrow_mut();
        let band = band as usize;
        (self.page).draw_band_with(
            &mut glyphs,
            band,
            offset as f32,
            selected,
            from..to,
            overlay,
        )
    }
}
