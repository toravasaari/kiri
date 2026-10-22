//! How large an image is shown, in pixels and in terminal cells.
//!
//! Images follow the browser: their natural size in CSS pixels, never
//! enlarged, and shrunk to the text width when they are wider. A terminal
//! does not say how its pixels compare to a browser's, so kiri takes one
//! text row, about 20 CSS pixels in a browser's default 16-pixel font, as
//! the measure: one CSS pixel is a twentieth of the cell height. Images then
//! keep the size they have next to the text in a browser, whatever the
//! display's scaling.
//!
//! A document can ask for another size, as HTML's `width` and `height` do.
//! A browser then shows that size, enlarged if need be, and kiri does too;
//! the text width still bounds it.

use crate::caps::Capabilities;
use crate::images::{ImageKey, Source};
use crate::model::Length;

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

/// The most pixels, wide and high, an image can be shown at: as many
/// cells as one image can cover each way.
pub fn largest(caps: &Capabilities) -> (u32, u32) {
    let (width, height) = cell_size(caps);
    let cells = u32::from(caps.max_image_cells);
    (cells * u32::from(width), cells * u32::from(height))
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

/// The size a document asks an image to be shown at. `None` leaves a side
/// to the image's natural size and shape. A percentage width is of the
/// room the image has, as a browser takes it of the block around the
/// image: in a list or quote, the width beside the bullet or bar, and at
/// most the image cell limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Wanted {
    pub width: Option<Length>,
    pub height: Option<Length>,
}

/// What the pixels of an image's natural size are on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// CSS pixels, as in a browser: a twentieth of the cell height each.
    Css,
    /// One image pixel per terminal pixel, as `kitten icat` shows images.
    Pixel,
}

/// The size of an image of `natural` CSS pixels, or of the size `wanted`
/// for it, in cells of `cell` pixels, at most `max_columns` wide and
/// `max_rows` high. `None` for an image without pixels or when there is no
/// room at all.
pub fn size(
    natural: (u32, u32),
    wanted: Wanted,
    cell: (u16, u16),
    max_columns: u16,
    max_rows: u16,
) -> Option<Size> {
    if natural.0 == 0 || natural.1 == 0 {
        return None;
    }
    let css = f64::from(cell.1) / CSS_PIXELS_PER_ROW;
    let pixels = |length| match length {
        Length::Pixels(pixels) => f64::from(pixels) * css,
        Length::Percent(percent) => {
            f64::from(max_columns) * f64::from(cell.0) * f64::from(percent) / 100.0
        }
    };
    let (natural_width, natural_height) = (f64::from(natural.0) * css, f64::from(natural.1) * css);
    let width = wanted.width.map(pixels);
    // A browser ignores a percentage height in running text: it would be
    // of a height nothing sets.
    let height = match wanted.height {
        Some(Length::Percent(_)) => None,
        height => height.map(pixels),
    };
    let (width, height) = match (width, height) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (width, width * natural_height / natural_width),
        (None, Some(height)) => (height * natural_width / natural_height, height),
        (None, None) => (natural_width, natural_height),
    };
    shrink_to((width, height), cell, max_columns, max_rows)
}

/// `size`, for an image whose natural size is in `unit`.
pub fn size_in(
    natural: (u32, u32),
    unit: Unit,
    cell: (u16, u16),
    max_columns: u16,
    max_rows: u16,
) -> Option<Size> {
    if natural.0 == 0 || natural.1 == 0 {
        return None;
    }
    let scale = match unit {
        Unit::Css => f64::from(cell.1) / CSS_PIXELS_PER_ROW,
        Unit::Pixel => 1.0,
    };
    let shown = (f64::from(natural.0) * scale, f64::from(natural.1) * scale);
    shrink_to(shown, cell, max_columns, max_rows)
}

/// An image of `shown` terminal pixels, shrunk to the room there is,
/// keeping its shape.
fn shrink_to(shown: (f64, f64), cell: (u16, u16), max_columns: u16, max_rows: u16) -> Option<Size> {
    if max_columns == 0 || max_rows == 0 || cell.0 == 0 || cell.1 == 0 {
        return None;
    }
    let (cell_width, cell_height) = (f64::from(cell.0), f64::from(cell.1));
    let (mut width, mut height) = shown;
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

/// The size `image` is shown at on its own, as an image file is: as
/// large as fits the window and no larger than its natural size, a raster
/// image one pixel per terminal pixel and others in CSS pixels. `reserved`
/// rows of the window are left to others, such as the shell's prompt
/// below the image. `None` when there is no room at all.
pub fn on_its_own(image: &Source, caps: &Capabilities, reserved: u16) -> Option<Size> {
    let unit = match image {
        Source::Raster(_) => Unit::Pixel,
        Source::Svg(_) | Source::Dense { .. } => Unit::Css,
    };
    let max_cells = caps.max_image_cells;
    let max_rows = caps.height.map_or(max_cells, |rows| {
        rows.saturating_sub(reserved).max(1).min(max_cells)
    });
    size_in(
        image.size(),
        unit,
        cell_size(caps),
        caps.width.min(max_cells),
        max_rows,
    )
}

/// An image shown as a block: which one, and at what size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Placement {
    /// Which image, the key into `Images`.
    pub key: ImageKey,
    pub size: Size,
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: (u16, u16) = (10, 20);

    #[test]
    fn an_image_is_shown_at_most_as_many_cells_as_it_can_cover() {
        let caps = Capabilities {
            max_image_cells: 300,
            cell_pixels: Some((9, 18)),
            ..Capabilities::kitty(80)
        };
        assert_eq!(largest(&caps), (2700, 5400));
        // The cell size assumed when the terminal tells none.
        let caps = Capabilities {
            cell_pixels: None,
            ..caps
        };
        assert_eq!(largest(&caps), (3000, 6000));
    }
    const NATURAL: Wanted = Wanted {
        width: None,
        height: None,
    };

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
        assert_eq!(
            size((320, 240), NATURAL, CELL, 100, 100),
            size_of(320, 240, 32, 12)
        );
    }

    #[test]
    fn one_css_pixel_is_a_twentieth_of_the_cell_height() {
        assert_eq!(
            size((320, 240), NATURAL, (20, 40), 100, 100),
            size_of(640, 480, 32, 12)
        );
        assert_eq!(
            size((320, 240), NATURAL, (8, 16), 100, 100),
            size_of(256, 192, 32, 12)
        );
    }

    #[test]
    fn keeps_one_image_pixel_per_terminal_pixel() {
        assert_eq!(
            size_in((320, 240), Unit::Pixel, (20, 40), 100, 100),
            size_of(320, 240, 16, 6)
        );
        assert_eq!(
            size_in((320, 240), Unit::Pixel, (20, 40), 10, 100),
            size_of(200, 150, 10, 4)
        );
    }

    #[test]
    fn never_enlarges_a_small_image() {
        assert_eq!(
            size((16, 16), NATURAL, CELL, 100, 100),
            size_of(16, 16, 2, 1)
        );
    }

    #[test]
    fn shrinks_a_wide_image_to_the_columns_keeping_its_shape() {
        assert_eq!(
            size((1600, 200), NATURAL, CELL, 44, 100),
            size_of(440, 55, 44, 3)
        );
    }

    #[test]
    fn an_indented_image_shrinks_to_the_room_left() {
        assert_eq!(
            size((320, 240), NATURAL, CELL, 20, 100),
            size_of(200, 150, 20, 8)
        );
    }

    #[test]
    fn shrinks_a_tall_image_to_the_rows() {
        assert_eq!(
            size((200, 800), NATURAL, CELL, 100, 10),
            size_of(50, 200, 5, 10)
        );
    }

    #[test]
    fn rounds_to_whole_pixels_and_covers_every_partial_cell() {
        assert_eq!(
            size((25, 25), NATURAL, CELL, 100, 100),
            size_of(25, 25, 3, 2)
        );
        // 18.75 pixels round to 19, a little over two cells of 7 and one
        // of 15.
        assert_eq!(
            size((25, 25), NATURAL, (7, 15), 100, 100),
            size_of(19, 19, 3, 2)
        );
    }

    #[test]
    fn a_sliver_keeps_at_least_one_pixel() {
        assert_eq!(
            size((10000, 1), NATURAL, CELL, 44, 100),
            size_of(440, 1, 44, 1)
        );
    }

    #[test]
    fn an_image_without_pixels_or_room_has_no_size() {
        assert_eq!(size((0, 10), NATURAL, CELL, 44, 100), None);
        assert_eq!(size((10, 0), NATURAL, CELL, 44, 100), None);
        assert_eq!(size((10, 10), NATURAL, CELL, 0, 100), None);
        assert_eq!(size((10, 10), NATURAL, CELL, 44, 0), None);
    }

    fn wanted(width: Option<Length>, height: Option<Length>) -> Wanted {
        Wanted { width, height }
    }

    #[test]
    fn a_wanted_width_keeps_the_shape() {
        let width = wanted(Some(Length::Pixels(160)), None);
        assert_eq!(
            size((320, 240), width, CELL, 100, 100),
            size_of(160, 120, 16, 6)
        );
    }

    #[test]
    fn a_wanted_height_keeps_the_shape() {
        let height = wanted(None, Some(Length::Pixels(60)));
        assert_eq!(
            size((320, 240), height, CELL, 100, 100),
            size_of(80, 60, 8, 3)
        );
    }

    #[test]
    fn a_wanted_width_and_height_change_the_shape() {
        let both = wanted(Some(Length::Pixels(100)), Some(Length::Pixels(100)));
        assert_eq!(
            size((320, 240), both, CELL, 100, 100),
            size_of(100, 100, 10, 5)
        );
    }

    #[test]
    fn a_wanted_size_is_in_css_pixels() {
        let width = wanted(Some(Length::Pixels(160)), None);
        assert_eq!(
            size((320, 240), width, (20, 40), 100, 100),
            size_of(320, 240, 16, 6)
        );
    }

    #[test]
    fn a_wanted_size_enlarges_a_small_image() {
        let width = wanted(Some(Length::Pixels(64)), None);
        assert_eq!(size((16, 16), width, CELL, 100, 100), size_of(64, 64, 7, 4));
    }

    #[test]
    fn a_wanted_size_still_shrinks_to_the_room() {
        let both = wanted(Some(Length::Pixels(1000)), Some(Length::Pixels(100)));
        assert_eq!(size((10, 10), both, CELL, 40, 100), size_of(400, 40, 40, 2));
        let tall = wanted(None, Some(Length::Pixels(1000)));
        assert_eq!(
            size((10, 10), tall, CELL, 100, 10),
            size_of(200, 200, 20, 10)
        );
    }

    #[test]
    fn a_wanted_percentage_is_of_the_width() {
        let half = wanted(Some(Length::Percent(50)), None);
        assert_eq!(
            size((320, 240), half, CELL, 40, 100),
            size_of(200, 150, 20, 8)
        );
        let double = wanted(Some(Length::Percent(200)), None);
        assert_eq!(
            size((32, 24), double, CELL, 40, 100),
            size_of(400, 300, 40, 15)
        );
    }

    #[test]
    fn a_percentage_height_is_ignored() {
        let height = wanted(None, Some(Length::Percent(50)));
        assert_eq!(
            size((320, 240), height, CELL, 100, 100),
            size_of(320, 240, 32, 12)
        );
        let both = wanted(Some(Length::Pixels(160)), Some(Length::Percent(50)));
        assert_eq!(
            size((320, 240), both, CELL, 100, 100),
            size_of(160, 120, 16, 6)
        );
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
