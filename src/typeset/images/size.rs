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

use std::ops::Range;

use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::images::{ImageKey, Source};
use crate::typeset::model::Length;

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
    let cells = u32::from(caps.max_image_cells());
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
    if max_columns == 0 || max_rows == 0 || cell.0 == 0 || cell.1 == 0 {
        return None;
    }
    let max_width = f64::from(max_columns) * f64::from(cell.0);
    let shown = wanted_size(natural, wanted, cell, max_width)?;
    shrink_to(shown, cell, max_columns, max_rows)
}

/// The size in whole pixels of an image of `natural` CSS pixels, or of the
/// size `wanted` for it, as `size` gives it, but in a room `max_width` and
/// `max_height` pixels large, which need not be whole cells. A percentage
/// width is of `max_width`.
pub fn pixel_size(
    natural: (u32, u32),
    wanted: Wanted,
    cell: (u16, u16),
    (max_width, max_height): (f64, f64),
) -> Option<(u32, u32)> {
    if max_width < 1.0 || max_height < 1.0 || cell.0 == 0 || cell.1 == 0 {
        return None;
    }
    let (mut width, mut height) = wanted_size(natural, wanted, cell, max_width)?;
    if width > max_width {
        height *= max_width / width;
        width = max_width;
    }
    if height > max_height {
        width *= max_height / height;
        height = max_height;
    }
    Some((
        (width.round() as u32).max(1),
        (height.round() as u32).max(1),
    ))
}

/// The size in terminal pixels that `wanted` gives an image of `natural`
/// CSS pixels, before it is shrunk to the room; `None` for an image
/// without pixels. A percentage width is of `max_width`.
pub(crate) fn wanted_size(
    natural: (u32, u32),
    wanted: Wanted,
    cell: (u16, u16),
    max_width: f64,
) -> Option<(f64, f64)> {
    if natural.0 == 0 || natural.1 == 0 {
        return None;
    }
    let css = f64::from(cell.1) / CSS_PIXELS_PER_ROW;
    let pixels = |length| match length {
        Length::Pixels(pixels) => f64::from(pixels) * css,
        Length::Percent(percent) => max_width * f64::from(percent) / 100.0,
    };
    let (natural_width, natural_height) = (f64::from(natural.0) * css, f64::from(natural.1) * css);
    let width = wanted.width.map(pixels);
    // A browser ignores a percentage height in running text: it would be
    // of a height nothing sets.
    let height = match wanted.height {
        Some(Length::Percent(_)) => None,
        height => height.map(pixels),
    };
    Some(match (width, height) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (width, width * natural_height / natural_width),
        (None, Some(height)) => (height * natural_width / natural_height, height),
        (None, None) => (natural_width, natural_height),
    })
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
        Source::Raster(_) | Source::Animated(_) | Source::Text { .. } => Unit::Pixel,
        Source::Svg(_) | Source::Dense { .. } => Unit::Css,
    };
    on_its_own_at(image.size(), unit, caps, reserved)
}

/// [`on_its_own`] for an image of which only the `natural` size, in `unit`,
/// is known, as the header of a file tells it.
pub fn on_its_own_at(
    natural: (u32, u32),
    unit: Unit,
    caps: &Capabilities,
    reserved: u16,
) -> Option<Size> {
    let max_cells = caps.max_image_cells();
    let max_rows = caps.height.map_or(max_cells, |rows| {
        rows.saturating_sub(reserved).max(1).min(max_cells)
    });
    size_in(
        natural,
        unit,
        cell_size(caps),
        caps.width.min(max_cells),
        max_rows,
    )
}

/// How an image or formula in a line is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fit {
    /// Pixels of the whole image, transparent rows included.
    pub width: u32,
    pub height: u32,
    /// Its opaque rows at that size.
    pub opaque: Range<u32>,
    pub columns: u16,
    /// 1 in a row, 2 for a formula that makes its line two rows high, and
    /// for a tall image as many as reach from its top to the bottom of the
    /// text's cell.
    pub rows: u16,
    /// Pixels from the image's top to its baseline.
    pub baseline: u32,
}

/// Where the text's baseline is in a cell `ch` pixels high.
pub fn baseline_in(ch: u32) -> u32 {
    (f64::from(ch) * 0.8).round() as u32
}

/// The image of `natural` pixels, `opaque` fractions of whose height are
/// not transparent, scaled by `scale`.
fn scaled(natural: (u32, u32), opaque: &Range<f64>, scale: f64) -> (u32, u32, Range<u32>) {
    let width = ((f64::from(natural.0) * scale).round() as u32).max(1);
    let height = ((f64::from(natural.1) * scale).round() as u32).max(1);
    let edge = |fraction: f64| ((fraction * f64::from(height)).round() as u32).min(height);
    let (top, bottom) = (edge(opaque.start), edge(opaque.end));
    (width, height, top..bottom.max(top + 1).min(height).max(top))
}

/// The trimmed height of an image of `natural` pixels at full size.
fn trimmed(natural: (u32, u32), opaque: &Range<f64>) -> u32 {
    let rows = scaled(natural, opaque, 1.0).2;
    (rows.end - rows.start).max(1)
}

/// The image drawn at the largest scale at which its trimmed height is at
/// most `target`, so that rounding never makes it a pixel taller.
fn within_height(natural: (u32, u32), opaque: &Range<f64>, target: u32) -> (u32, u32, Range<u32>) {
    let target = target.max(1);
    let mut scale = f64::from(target) / f64::from(trimmed(natural, opaque));
    loop {
        let drawn = scaled(natural, opaque, scale);
        if drawn.2.end - drawn.2.start <= target || scale < 1e-3 {
            return drawn;
        }
        scale -= 0.25 / f64::from(natural.1.max(1));
    }
}

/// The image shrunk to the room `max_width` pixels wide, if it is wider.
fn within_width(natural: (u32, u32), max_width: u32) -> (u32, u32) {
    if natural.0 <= max_width {
        return natural;
    }
    let scale = f64::from(max_width) / f64::from(natural.0);
    let height = (f64::from(natural.1) * scale).round() as u32;
    (max_width, height.max(1))
}

fn columns_of(width: u32, cw: u16) -> u16 {
    width
        .div_ceil(u32::from(cw.max(1)))
        .min(u32::from(u16::MAX)) as u16
}

/// Fits an image of `natural` device pixels into a line. `opaque` is its
/// opaque rows as fractions of its height; `room` the cell, or
/// `(cw, ch * scale)` in a heading; `grow` whether the line may grow, which
/// it may not in a heading, where the image is shrunk into one row.
///
/// Rules, with `h` the trimmed height and `w` the width: an image higher
/// than three rows is tall and keeps its size, up to `max_cells` rows; one
/// that is a badge (`h` at most a row and `w` over twice `h`) or at most
/// 0.9 rows high is kept; any other is shrunk to between 0.9 and 0.75
/// rows, to the height that leaves the fewest pixels unused in its last
/// cell.
pub fn fit_image(
    natural: (u32, u32),
    opaque: Range<f64>,
    room: (u16, u16),
    max_columns: u16,
    max_cells: u16,
    grow: bool,
) -> Fit {
    let (cw, ch) = (u32::from(room.0.max(1)), u32::from(room.1.max(1)));
    let natural = within_width(natural, u32::from(max_columns.max(1)) * cw);
    let h = trimmed(natural, &opaque);
    let baseline_row = baseline_in(ch);
    let fit = |(width, height, rows): (u32, u32, Range<u32>), cells: u16| Fit {
        width,
        height,
        columns: columns_of(width, room.0),
        baseline: rows.end,
        opaque: rows,
        rows: cells,
    };
    if !grow {
        // One row, whatever the image is: shrunk to it when it is higher.
        if h > ch {
            return fit(within_height(natural, &opaque, ch), 1);
        }
    } else if h > 3 * ch {
        // A tall image: as many rows as it needs, at most `max_cells`.
        let most = (u32::from(max_cells.max(1)) - 1) * ch + baseline_row;
        let drawn = if h > most {
            within_height(natural, &opaque, most)
        } else {
            scaled(natural, &opaque, 1.0)
        };
        let shown = drawn.2.end - drawn.2.start;
        let rows = shown.saturating_sub(baseline_row).div_ceil(ch) + 1;
        return fit(drawn, rows as u16);
    }
    let band = ch - (f64::from(ch) * 0.1).round() as u32;
    let width = natural.0;
    if (h <= ch && width > 2 * h) || h <= band {
        return fit(scaled(natural, &opaque, 1.0), 1);
    }
    // Shrunk: the height from `band` down to three quarters of a row whose
    // width leaves the fewest pixels unused in its last cell; the larger
    // height wins a tie.
    let lowest = (f64::from(ch) * 0.75).round() as u32;
    let mut best: Option<(u32, u32)> = None;
    for t in (lowest..=band).rev() {
        let wide = (f64::from(width) * f64::from(t) / f64::from(h)).round() as u32;
        let unused = wide.div_ceil(cw) * cw - wide;
        if best.is_none_or(|(least, _)| unused < least) {
            best = Some((unused, t));
        }
    }
    let t = best.map_or(band, |(_, t)| t);
    fit(within_height(natural, &opaque, t), 1)
}

/// Fits a formula of `natural` device pixels, drawn in text style, into a
/// line; `depth` is the fraction of its untrimmed height below its
/// baseline, `opaque` as for `fit_image`.
///
/// One that is at most a row high is kept; one that would fit a row by
/// shrinking to 0.85 of its size at the least is drawn smaller; any other
/// makes its line two rows high, at its size, or shrunk to two rows when it
/// is higher. Where the line may not grow, it is shrunk to one row.
pub fn fit_formula(
    natural: (u32, u32),
    opaque: Range<f64>,
    depth: f64,
    room: (u16, u16),
    max_columns: u16,
    grow: bool,
) -> Fit {
    let (cw, ch) = (u32::from(room.0.max(1)), u32::from(room.1.max(1)));
    let natural = within_width(natural, u32::from(max_columns.max(1)) * cw);
    let h = trimmed(natural, &opaque);
    let (drawn, rows) = if h <= ch {
        (scaled(natural, &opaque, 1.0), 1)
    } else if !grow || f64::from(ch) / f64::from(h) >= 0.85 {
        (within_height(natural, &opaque, ch), 1)
    } else if h <= 2 * ch {
        (scaled(natural, &opaque, 1.0), 2)
    } else {
        (within_height(natural, &opaque, 2 * ch), 2)
    };
    let (width, height, rows_of_ink) = drawn;
    let below = (depth.clamp(0.0, 1.0) * f64::from(height)).round() as u32;
    Fit {
        width,
        height,
        columns: columns_of(width, room.0),
        baseline: height - below.min(height),
        opaque: rows_of_ink,
        rows,
    }
}

/// An image shown as a block: which one, and at what size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Placement {
    /// Which image, the key into `Images`.
    pub key: ImageKey,
    pub size: Size,
    /// The image itself, or what stands in for it while it loads.
    pub shows: Shows,
    /// Where the image lies on its canvas of cells, in pixels from the
    /// canvas's top left corner: `(0, 0)` for a block, which fills its
    /// cells from there. An image in a line lies elsewhere in its cells;
    /// a negative offset cuts off the rows above or the columns left of it.
    pub at: (i32, i32),
    /// The frame a marked image in a line has.
    pub frame: Option<Frame>,
}

/// A frame along the edge of the cells of some rows of an image's canvas.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Frame {
    pub color: Color,
    /// The rows of the canvas it goes round, of the cells that the
    /// image itself is on.
    pub rows: Range<u16>,
    /// The animated picture the frame is for, when the image is a part of
    /// a page that holds several: the frame goes round that one.
    pub picture: Option<ImageKey>,
}

impl Placement {
    /// The placement of an image shown as a block, at the corner of its
    /// cells.
    pub fn block(key: ImageKey, size: Size, shows: Shows) -> Self {
        Placement {
            key,
            size,
            shows,
            at: (0, 0),
            frame: None,
        }
    }
}

/// What a placement shows of its image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Shows {
    /// The image.
    #[default]
    Image,
    /// Its placeholder while it loads: the old image dimmed.
    Placeholder,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An image of `w` × `h` pixels without transparent rows, in a row of
    /// cells of 9 × 20.
    fn image(w: u32, h: u32) -> Fit {
        fit_image((w, h), 0.0..1.0, (9, 20), 100, 30, true)
    }

    fn sized(fit: &Fit) -> (u32, u32, u16, u16) {
        (fit.width, fit.height, fit.columns, fit.rows)
    }

    #[test]
    fn a_badge_an_icon_and_a_low_image_are_kept() {
        assert_eq!(sized(&image(89, 20)), (89, 20, 10, 1));
        assert_eq!(sized(&image(16, 16)), (16, 16, 2, 1));
        assert_eq!(image(16, 16).baseline, 16);
    }

    #[test]
    fn an_image_that_is_too_high_for_a_row_is_shrunk_to_leave_little_of_its_last_cell_unused() {
        assert_eq!(sized(&image(41, 24)), (27, 16, 3, 1));
        assert_eq!(sized(&image(28, 27)), (18, 17, 2, 1));
        assert_eq!(sized(&image(54, 43)), (23, 18, 3, 1));
    }

    #[test]
    fn transparent_rows_are_not_part_of_an_images_size() {
        let fit = fit_image((20, 24), 2.0 / 24.0..22.0 / 24.0, (9, 20), 100, 30, true);
        assert_eq!(fit.width, 18);
        assert_eq!(fit.opaque.end - fit.opaque.start, 18);
        assert_eq!(fit.height, 22);
        assert_eq!(fit.baseline, fit.opaque.end);
    }

    #[test]
    fn a_tall_image_keeps_its_size_and_has_rows_to_the_text_cell_s_bottom() {
        assert_eq!(sized(&image(96, 64)), (96, 64, 11, 4));
        // Shrunk to the text's width first, then tall.
        let fit = fit_image((400, 300), 0.0..1.0, (9, 20), 20, 30, true);
        assert_eq!(sized(&fit), (180, 135, 20, 7));
        // At most as many rows as an image may cover.
        let fit = fit_image((90, 900), 0.0..1.0, (9, 20), 100, 10, true);
        assert_eq!(fit.rows, 10);
        assert!(fit.height <= 9 * 20 + 16);
    }

    #[test]
    fn in_a_heading_an_image_is_shrunk_into_one_row() {
        let fit = fit_image((96, 64), 0.0..1.0, (9, 40), 100, 30, false);
        assert_eq!(sized(&fit), (60, 40, 7, 1));
    }

    fn formula(h: u32) -> Fit {
        fit_formula((30, h), 0.0..1.0, 0.25, (9, 20), 100, true)
    }

    #[test]
    fn a_formula_stays_shrinks_into_the_row_or_makes_its_line_two_rows() {
        let stays = formula(16);
        assert_eq!((stays.height, stays.rows), (16, 1));
        assert_eq!(stays.baseline, 12);
        let shrunk = formula(22);
        assert!(shrunk.height <= 20 && shrunk.rows == 1);
        let two = formula(27);
        assert_eq!((two.height, two.rows), (27, 2));
        let shrunk_two = formula(44);
        assert!(shrunk_two.height <= 40 && shrunk_two.rows == 2);
        // In a heading everything fits one row.
        let heading = fit_formula((30, 44), 0.0..1.0, 0.0, (9, 40), 100, false);
        assert!(heading.height <= 40 && heading.rows == 1);
    }

    const CELL: (u16, u16) = (10, 20);

    #[test]
    fn an_image_is_shown_at_most_as_many_cells_as_it_can_cover() {
        let caps = Capabilities {
            graphics: crate::typeset::caps::Graphics::kitty_cells(300),
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
    fn pixel_sizes_are_those_of_cells_in_a_room_of_any_width() {
        // In a whole number of cells, the same as `size`.
        assert_eq!(
            pixel_size((1600, 200), NATURAL, CELL, (440.0, 5940.0)),
            Some((440, 55))
        );
        // In a room that is not, the image fills it exactly.
        assert_eq!(
            pixel_size((1600, 200), NATURAL, CELL, (672.0, 5940.0)),
            Some((672, 84))
        );
        // A small image keeps its size, and a tall one is cut to the height.
        assert_eq!(
            pixel_size((16, 16), NATURAL, CELL, (672.0, 5940.0)),
            Some((16, 16))
        );
        assert_eq!(
            pixel_size((200, 800), NATURAL, CELL, (672.0, 200.0)),
            Some((50, 200))
        );
        // A percentage is of the room.
        let half = Wanted {
            width: Some(Length::Percent(50)),
            height: None,
        };
        assert_eq!(
            pixel_size((100, 50), half, CELL, (672.0, 5940.0)),
            Some((336, 168))
        );
        assert_eq!(pixel_size((0, 50), NATURAL, CELL, (672.0, 5940.0)), None);
        assert_eq!(pixel_size((10, 10), NATURAL, CELL, (0.0, 5940.0)), None);
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
