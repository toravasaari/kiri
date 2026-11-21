//! Glyphs drawn once. Drawing a glyph's outline is most of what drawing
//! text costs, and the same glyph comes again and again, so the coverage of
//! a monochrome glyph is kept and laid on the canvas in the colour of the
//! text each time it is wanted. A glyph is the same where it falls at the
//! same sixteenth of a pixel in each direction, so its place is rounded to
//! that, which moves an edge by at most a sixteenth of a pixel.
//!
//! Where two glyphs overlap, their coverage now combines as two draws one
//! over the other, where drawing a run as one outline fills the overlap
//! once; this is part of the difference from drawing every run as one path.
//!
//! A colour glyph (COLR) is drawn by [`colr`](super::colr) each time, from
//! the font; only when that fails does its outline stand in for it.

use std::collections::HashMap;
use std::fmt;

use resvg::tiny_skia::{FillRule, Mask, Path, Pixmap, Transform};

/// What the glyphs held may take, in bytes: each one's mask and its own
/// struct, not its key or the table's spare room. Past it the cache starts
/// over.
const BUDGET: usize = 32 << 20;

/// The parts of a pixel a glyph's place is rounded to.
const STEPS: f32 = 16.0;

/// The font, size and slant a run of glyphs is drawn in, with a hash of the
/// variation coordinates. The font's blob id is in it, so a change of fonts
/// finds no glyph of the old ones.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Face {
    blob: u64,
    index: u32,
    size: u32,
    skew: u32,
    coords: u64,
}

impl Face {
    /// The face of font `index` in the blob `blob`, at `size` pixels and
    /// slanted by `skew`, with its normalized variation `coords`.
    pub fn new(
        blob: u64,
        index: u32,
        size: f32,
        skew: f32,
        coords: impl IntoIterator<Item = i16>,
    ) -> Self {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        for coord in coords {
            coord.hash(&mut hasher);
        }
        Self {
            blob,
            index,
            size: size.to_bits(),
            skew: skew.to_bits(),
            coords: hasher.finish(),
        }
    }
}

/// Where a glyph's origin is: the pixel it is in, and the sixteenths of a
/// pixel after the pixel's corner, in x and in y.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    whole: (i32, i32),
    steps: (u8, u8),
}

impl Origin {
    /// The origin at (`x`, `y`), rounded to the nearest sixteenth.
    pub fn of(x: f32, y: f32) -> Self {
        let ((wx, sx), (wy, sy)) = (split(x), split(y));
        Self {
            whole: (wx, wy),
            steps: (sx, sy),
        }
    }

    /// The offset into its pixel that the glyph is drawn at.
    fn offset(self) -> (f32, f32) {
        (
            f32::from(self.steps.0) / STEPS,
            f32::from(self.steps.1) / STEPS,
        )
    }
}

/// `at` as a whole pixel and a number of sixteenths, rounded to the
/// nearest; a fraction that rounds up to the next pixel is in it.
fn split(at: f32) -> (i32, u8) {
    let whole = at.floor();
    let step = ((at - whole) * STEPS).round() as u8;
    match f32::from(step) == STEPS {
        true => (whole as i32 + 1, 0),
        false => (whole as i32, step),
    }
}

/// A glyph of a face: whether it is a colour glyph, and its outline's
/// coverage when it has an outline.
pub struct Glyph {
    colour: bool,
    mask: Option<Covered>,
}

/// A glyph's coverage, with its top left counted from the corner of the pixel
/// its origin is in.
struct Covered {
    left: i32,
    top: i32,
    mask: Mask,
}

impl Glyph {
    /// A glyph drawn from `path`, which has the glyph's origin at the offset
    /// into the pixel at (0, 0) that [`GlyphCache::draw`] gives; `colour`
    /// when the font has a colour glyph for it.
    pub fn new(colour: bool, path: Option<Path>) -> Self {
        Self {
            colour,
            mask: path.and_then(|path| covered(&path)),
        }
    }

    /// What the glyph takes of the cache's budget.
    fn bytes(&self) -> usize {
        size_of::<Self>()
            + self
                .mask
                .as_ref()
                .map_or(0, |covered| covered.mask.data().len())
    }
}

/// The coverage of `path`, or none when it covers nothing.
fn covered(path: &Path) -> Option<Covered> {
    let bounds = path.bounds();
    let left = bounds.left().floor() as i32;
    let top = bounds.top().floor() as i32;
    let width = (bounds.right().ceil() as i32 - left).max(1) as u32;
    let height = (bounds.bottom().ceil() as i32 - top).max(1) as u32;
    let mut mask = Mask::new(width, height)?;
    let shift = Transform::from_translate(-left as f32, -top as f32);
    mask.fill_path(path, FillRule::Winding, true, shift);
    Some(Covered { left, top, mask })
}

/// Glyphs by face, glyph and place, kept up to a budget of bytes.
pub struct GlyphCache {
    glyphs: HashMap<(Face, u32, (u8, u8)), Glyph>,
    bytes: usize,
    budget: usize,
    hits: u64,
    made: u64,
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::with_budget(BUDGET)
    }
}

impl fmt::Debug for GlyphCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlyphCache")
            .field("glyphs", &self.glyphs.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl GlyphCache {
    /// A cache that holds up to `budget` bytes of masks.
    pub fn with_budget(budget: usize) -> Self {
        Self {
            glyphs: HashMap::new(),
            bytes: 0,
            budget,
            hits: 0,
            made: 0,
        }
    }

    /// How many glyphs were found held, and how many were made.
    pub fn counts(&self) -> (u64, u64) {
        (self.hits, self.made)
    }

    /// The bytes the glyphs held take.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Draws glyph `glyph` of `face` with its origin at `origin` on `target`
    /// in `color`, inside `clip` when given, as an opaque fill would.
    ///
    /// A glyph not held is made by `make`, which gets the offset into its
    /// pixel the outline is to start at. A colour glyph is drawn by `colour`
    /// first, and is left at that when it says it drew it; otherwise the
    /// glyph's outline is drawn.
    pub fn draw(
        &mut self,
        target: &mut Pixmap,
        (face, glyph): (Face, u32),
        origin: Origin,
        (color, clip): ([u8; 3], Option<&Mask>),
        make: impl FnOnce((f32, f32)) -> Glyph,
        colour: impl FnOnce(&mut Pixmap) -> bool,
    ) {
        let key = (face, glyph, origin.steps);
        let made;
        let held = if self.glyphs.contains_key(&key) {
            self.hits += 1;
            &self.glyphs[&key]
        } else {
            self.made += 1;
            made = make(origin.offset());
            let bytes = made.bytes();
            if bytes > self.budget {
                &made
            } else {
                if self.bytes + bytes > self.budget {
                    self.glyphs.clear();
                    self.bytes = 0;
                }
                self.bytes += bytes;
                self.glyphs.entry(key).or_insert(made)
            }
        };
        if held.colour && colour(target) {
            return;
        }
        if let Some(covered) = &held.mask {
            blend(target, covered, origin.whole, color, clip);
        }
    }
}

/// `value` over 255, rounded as tiny-skia does in its fills.
fn div255(value: u32) -> u32 {
    (value + 255) >> 8
}

/// Lays `covered` on `target` in `color` with its origin in the pixel at
/// (`x`, `y`), inside `clip` when given, as an opaque fill does: each pixel
/// keeps what the coverage does not take.
fn blend(
    target: &mut Pixmap,
    covered: &Covered,
    (x, y): (i32, i32),
    color: [u8; 3],
    clip: Option<&Mask>,
) {
    debug_assert!(
        clip.is_none_or(|clip| (clip.width(), clip.height()) == (target.width(), target.height())),
        "the clip is made at the size of the canvas"
    );
    let (target_width, target_height) = (target.width() as i32, target.height() as i32);
    let (width, height) = (covered.mask.width() as i32, covered.mask.height() as i32);
    let (left, top) = (x + covered.left, y + covered.top);
    let (first_column, end_column) = (left.max(0), (left + width).min(target_width));
    let (first_row, end_row) = (top.max(0), (top + height).min(target_height));
    if first_column >= end_column || first_row >= end_row {
        return;
    }
    let coverage = covered.mask.data();
    let clip = clip.map(Mask::data);
    let pixels = target.data_mut();
    let [red, green, blue] = color.map(u32::from);
    for row in first_row..end_row {
        let mask_row = ((row - top) * width) as usize;
        let target_row = (row * target_width) as usize;
        for column in first_column..end_column {
            let mut cover = u32::from(coverage[mask_row + (column - left) as usize]);
            let at = target_row + column as usize;
            if let Some(clip) = clip {
                cover = div255(cover * u32::from(clip[at]));
            }
            if cover == 0 {
                continue;
            }
            let pixel = &mut pixels[at * 4..at * 4 + 4];
            if cover == 255 {
                pixel.copy_from_slice(&[color[0], color[1], color[2], 255]);
                continue;
            }
            let keep = 255 - cover;
            pixel[0] = div255(u32::from(pixel[0]) * keep + red * cover) as u8;
            pixel[1] = div255(u32::from(pixel[1]) * keep + green * cover) as u8;
            pixel[2] = div255(u32::from(pixel[2]) * keep + blue * cover) as u8;
            pixel[3] = div255(u32::from(pixel[3]) * keep + 255 * cover) as u8;
        }
    }
}

#[cfg(test)]
mod tests;
