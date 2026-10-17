//! How large an image is shown, in pixels and in terminal cells.
//!
//! Images follow the browser: their natural size in CSS pixels, never
//! enlarged, and shrunk to the text width when they are wider. A terminal
//! does not say how its pixels compare to a browser's, so kiri takes one
//! text row, about 20 CSS pixels in a browser's default 16-pixel font, as
//! the measure: one CSS pixel is a twentieth of the cell height. Images then
//! keep the size they have next to the text in a browser, whatever the
//! display's scaling.

use crate::term::Capabilities;

/// The cell size assumed when the terminal does not report one.
pub const DEFAULT_CELL: (u16, u16) = (10, 20);

/// CSS pixels in one text row.
const CSS_PIXELS_PER_ROW: f64 = 20.0;

/// The size of one cell in pixels, width and height.
pub fn cell_size(caps: &Capabilities) -> (u16, u16) {
    caps.cell_pixels
        .filter(|&(width, height)| width > 0 && height > 0)
        .unwrap_or(DEFAULT_CELL)
}

/// The size an image is shown at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    /// The image itself in terminal pixels.
    pub width: u32,
    pub height: u32,
    /// The cells it covers. The image fills them from the top left corner,
    /// and less than one cell is left over to the right and below.
    pub columns: u16,
    pub rows: u16,
}

/// The size of an image of `natural` pixels in cells of `cell` pixels, at
/// most `max_columns` wide and `max_rows` high. `None` for an image without
/// pixels or when there is no room at all.
pub fn size(
    natural: (u32, u32),
    cell: (u16, u16),
    max_columns: u16,
    max_rows: u16,
) -> Option<Size> {
    let (cell_width, cell_height) = (f64::from(cell.0), f64::from(cell.1));
    if natural.0 == 0
        || natural.1 == 0
        || max_columns == 0
        || max_rows == 0
        || cell.0 == 0
        || cell.1 == 0
    {
        return None;
    }
    let scale = cell_height / CSS_PIXELS_PER_ROW;
    let mut width = f64::from(natural.0) * scale;
    let mut height = f64::from(natural.1) * scale;
    // Shrink to the room there is, keeping the shape.
    let max_width = f64::from(max_columns) * cell_width;
    if width > max_width {
        height *= max_width / width;
        width = max_width;
    }
    let max_height = f64::from(max_rows) * cell_height;
    if height > max_height {
        width *= max_height / height;
        height = max_height;
    }
    let width = (width.round() as u32).max(1);
    let height = (height.round() as u32).max(1);
    Some(Size {
        width,
        height,
        columns: width.div_ceil(u32::from(cell.0)) as u16,
        rows: height.div_ceil(u32::from(cell.1)) as u16,
    })
}

/// An image shown as a block: which one, and at what size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Placement {
    /// The URL the document gives, the key into `Images`.
    pub url: String,
    pub size: Size,
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: (u16, u16) = (10, 20);

    fn size_of(width: u32, height: u32, columns: u16, rows: u16) -> Option<Size> {
        Some(Size {
            width,
            height,
            columns,
            rows,
        })
    }

    #[test]
    fn keeps_the_natural_size_in_css_pixels() {
        assert_eq!(size((320, 240), CELL, 100, 100), size_of(320, 240, 32, 12));
    }

    #[test]
    fn one_css_pixel_is_a_twentieth_of_the_cell_height() {
        assert_eq!(
            size((320, 240), (20, 40), 100, 100),
            size_of(640, 480, 32, 12)
        );
        assert_eq!(
            size((320, 240), (8, 16), 100, 100),
            size_of(256, 192, 32, 12)
        );
    }

    #[test]
    fn never_enlarges_a_small_image() {
        assert_eq!(size((16, 16), CELL, 100, 100), size_of(16, 16, 2, 1));
    }

    #[test]
    fn shrinks_a_wide_image_to_the_columns_keeping_its_shape() {
        assert_eq!(size((1600, 200), CELL, 44, 100), size_of(440, 55, 44, 3));
    }

    #[test]
    fn an_indented_image_shrinks_to_the_room_left() {
        assert_eq!(size((320, 240), CELL, 20, 100), size_of(200, 150, 20, 8));
    }

    #[test]
    fn shrinks_a_tall_image_to_the_rows() {
        assert_eq!(size((200, 800), CELL, 100, 10), size_of(50, 200, 5, 10));
    }

    #[test]
    fn rounds_to_whole_pixels_and_covers_every_partial_cell() {
        assert_eq!(size((25, 25), CELL, 100, 100), size_of(25, 25, 3, 2));
        // 18.75 pixels round to 19, a little over two cells of 7 and one
        // of 15.
        assert_eq!(size((25, 25), (7, 15), 100, 100), size_of(19, 19, 3, 2));
    }

    #[test]
    fn a_sliver_keeps_at_least_one_pixel() {
        assert_eq!(size((10000, 1), CELL, 44, 100), size_of(440, 1, 44, 1));
    }

    #[test]
    fn an_image_without_pixels_or_room_has_no_size() {
        assert_eq!(size((0, 10), CELL, 44, 100), None);
        assert_eq!(size((10, 0), CELL, 44, 100), None);
        assert_eq!(size((10, 10), CELL, 0, 100), None);
        assert_eq!(size((10, 10), CELL, 44, 0), None);
    }

    #[test]
    fn assumes_ten_by_twenty_pixel_cells_when_unknown() {
        let mut caps = Capabilities::kitty(80);
        assert_eq!(cell_size(&caps), (10, 20));
        caps.cell_pixels = Some((9, 18));
        assert_eq!(cell_size(&caps), (9, 18));
        caps.cell_pixels = Some((0, 18));
        assert_eq!(cell_size(&caps), (10, 20));
    }
}
