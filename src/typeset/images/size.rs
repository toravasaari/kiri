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

/// How many terminal pixels one CSS pixel is on a terminal whose cells are
/// `cell` pixels: a twentieth of the cell height.
pub fn css_pixel(cell: (u16, u16)) -> f64 {
    f64::from(cell.1) / CSS_PIXELS_PER_ROW
}

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
    let shown = wanted_size(natural, wanted, css_pixel(cell), max_width)?;
    shrink_to(shown, cell, max_columns, max_rows)
}

/// The size in whole pixels of an image of `natural` CSS pixels, or of the
/// size `wanted` for it, as `size` gives it, but in a room `max_width` and
/// `max_height` pixels large, which need not be whole cells. A CSS pixel is
/// `css` terminal pixels. A percentage width is of `max_width`.
pub fn pixel_size(
    natural: (u32, u32),
    wanted: Wanted,
    css: f64,
    (max_width, max_height): (f64, f64),
) -> Option<(u32, u32)> {
    if max_width < 1.0 || max_height < 1.0 || css.is_nan() || css <= 0.0 {
        return None;
    }
    let (mut width, mut height) = wanted_size(natural, wanted, css, max_width)?;
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
/// without pixels. A CSS pixel is `css` terminal pixels. A percentage width
/// is of `max_width`.
pub(crate) fn wanted_size(
    natural: (u32, u32),
    wanted: Wanted,
    css: f64,
    max_width: f64,
) -> Option<(f64, f64)> {
    if natural.0 == 0 || natural.1 == 0 {
        return None;
    }
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
        Unit::Css => css_pixel(cell),
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
    on_its_own_at(image.size(), image.unit(), caps, reserved)
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
/// most `target`, so that rounding never makes it a pixel taller. Ends at the
/// latest when the scale reaches zero, which draws one pixel.
fn within_height(natural: (u32, u32), opaque: &Range<f64>, target: u32) -> (u32, u32, Range<u32>) {
    let target = target.max(1);
    let mut scale = f64::from(target) / f64::from(trimmed(natural, opaque));
    loop {
        let drawn = scaled(natural, opaque, scale);
        if drawn.2.end - drawn.2.start <= target || scale <= 0.0 {
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
mod props;

#[cfg(test)]
mod tests;
