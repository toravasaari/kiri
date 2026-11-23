//! Laid-out text drawn as pixels: its glyphs' outlines, through skrifa, filled
//! with tiny-skia. Chips under its inline code and the rules and bars around
//! blocks are drawn too.
//!
//! What is not drawn stays transparent, so the page shows through.

use image::RgbaImage;
use resvg::tiny_skia::{
    self, BlendMode, Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap,
    PixmapPaint, Point, PremultipliedColorU8, Rect, SpreadMode, Transform,
};
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::types::F2Dot14;
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::typeset::reading::colr;
use crate::typeset::reading::glyph_cache::{Face, Glyph, GlyphCache, Origin};
use crate::typeset::reading::pixels::Pixels;
use crate::typeset::reading::text::{Brush, Paragraph};
use parley::PositionedLayoutItem;

/// How far inline code's chip reaches above the baseline, how tall it is
/// and how far it reaches past the code on each side, in font sizes of the
/// text around it; and the radius of its corners. It sits a little lower
/// than the line, as the code is smaller than its text.
const CHIP_ABOVE: f32 = 0.95;
const CHIP_HEIGHT: f32 = 1.25;
const CHIP_PAD: f32 = 0.12;
const CHIP_RADIUS: f32 = 0.25;

/// A place in a text drawn over: a box in `background`, if it has one,
/// with the text in it drawn again in `foreground`, or in its own colours
/// when there is none.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMark {
    pub range: std::ops::Range<usize>,
    pub foreground: Option<Brush>,
    pub background: Option<Brush>,
}

/// A picture being drawn, transparent where nothing is, with the glyphs
/// that it and the pictures drawn before it share.
pub struct Canvas<'a> {
    pub(super) pixmap: Pixmap,
    glyphs: &'a mut GlyphCache,
}

impl<'a> Canvas<'a> {
    /// A transparent canvas `width` by `height` pixels, at least one each,
    /// that draws its glyphs through `glyphs`.
    pub fn new(width: u32, height: u32, glyphs: &'a mut GlyphCache) -> Self {
        Self {
            pixmap: Pixmap::new(width.max(1), height.max(1)).expect("a size of at least 1"),
            glyphs,
        }
    }

    /// A canvas `width` by `height` pixels in `background`, or transparent
    /// where there is none.
    pub fn on(
        width: u32,
        height: u32,
        background: Option<Brush>,
        glyphs: &'a mut GlyphCache,
    ) -> Self {
        let mut canvas = Self::new(width, height, glyphs);
        if let Some([red, green, blue]) = background {
            canvas.pixmap.fill(Color::from_rgba8(red, green, blue, 255));
        }
        canvas
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// Draws `paragraph` with the top left of its text at (`x`, `y`), on
    /// chips of `chip` behind its inline code when given.
    pub fn text(&mut self, paragraph: &Paragraph, x: f32, y: f32, chip: Option<Brush>) {
        self.text_marked(paragraph, x, y, chip, &[]);
    }

    /// `text`, with `marks` laid over it.
    pub fn text_marked(
        &mut self,
        paragraph: &Paragraph,
        x: f32,
        y: f32,
        chip: Option<Brush>,
        marks: &[TextMark],
    ) {
        if let Some(chip) = chip {
            let size = paragraph.look.size;
            let lines = paragraph.lines();
            for range in paragraph.code() {
                for part in paragraph.boxes(range.clone()) {
                    let baseline = y + lines[part.line].baseline;
                    let pad = CHIP_PAD * size;
                    self.round_rect(
                        x + part.left - pad,
                        baseline - CHIP_ABOVE * size,
                        part.right - part.left + 2.0 * pad,
                        CHIP_HEIGHT * size,
                        CHIP_RADIUS * size,
                        chip,
                    );
                }
            }
        }
        for (range, background) in paragraph.backgrounds() {
            let size = paragraph.look.size;
            let lines = paragraph.lines();
            for part in paragraph.boxes(range.clone()) {
                let baseline = y + lines[part.line].baseline;
                self.rect(
                    x + part.left,
                    baseline - CHIP_ABOVE * size,
                    part.right - part.left,
                    CHIP_HEIGHT * size,
                    *background,
                );
            }
        }
        self.glyphs_over(paragraph, x, y, None);
        self.marks(paragraph, x, y, marks);
    }

    /// Lays `marks` over the text drawn at (`x`, `y`): a box of its
    /// background colour on each line a mark covers, and the glyphs in the
    /// boxes drawn again in their foreground colour. Marks of one colour
    /// share one mask and one pass over the glyphs.
    fn marks(&mut self, paragraph: &Paragraph, x: f32, y: f32, marks: &[TextMark]) {
        if marks.is_empty() {
            return;
        }
        let lines = paragraph.lines();
        // The boxes first, so that no glyph pass is covered by another's.
        let mut groups: Vec<(Option<Brush>, Vec<Rect>)> = Vec::new();
        for mark in marks {
            let at = match groups
                .iter()
                .position(|(foreground, _)| *foreground == mark.foreground)
            {
                Some(at) => at,
                None => {
                    groups.push((mark.foreground, Vec::new()));
                    groups.len() - 1
                }
            };
            for part in paragraph.boxes(mark.range.clone()) {
                let Some(line) = lines.get(part.line) else {
                    continue;
                };
                let (left, top) = (x + part.left, y + line.top);
                let (width, height) = (part.right - part.left, line.height);
                if let Some(background) = mark.background {
                    self.rect(left, top, width, height, background);
                }
                if let Some(rect) = Rect::from_xywh(left, top, width, height) {
                    groups[at].1.push(rect);
                }
            }
        }
        for (foreground, boxes) in groups {
            let Some(mut mask) = tiny_skia::Mask::new(self.pixmap.width(), self.pixmap.height())
            else {
                continue;
            };
            for rect in boxes {
                mask.fill_path(
                    &PathBuilder::from_rect(rect),
                    FillRule::Winding,
                    false,
                    Transform::identity(),
                );
            }
            self.glyphs_over(paragraph, x, y, Some((foreground, &mask)));
        }
    }

    /// The glyphs of `paragraph` at (`x`, `y`), in the colour and inside the
    /// mask of `over` when it is given.
    fn glyphs_over(
        &mut self,
        paragraph: &Paragraph,
        x: f32,
        y: f32,
        over: Option<(Option<Brush>, &tiny_skia::Mask)>,
    ) {
        // Lines that do not reach the canvas are left out, with a font
        // size of room for what reaches past their boxes.
        let reach = paragraph.look.size;
        let height = self.pixmap.height() as f32;
        for (piece, line) in (paragraph.pieces.iter())
            .flat_map(|piece| piece.layout.lines().map(move |line| (piece, line)))
        {
            let (start, y) = (piece.start, y + piece.top);
            let metrics = line.metrics();
            if y + metrics.block_max_coord + reach < 0.0
                || y + metrics.block_min_coord - reach > height
            {
                continue;
            }
            // The shaped run last drawn on this line, and how many of its
            // glyphs the glyph runs so far took.
            let mut taken = (usize::MAX, 0);
            for item in line.items() {
                let glyphs = match item {
                    PositionedLayoutItem::GlyphRun(glyphs) => glyphs,
                    PositionedLayoutItem::InlineBox(placed) => {
                        let picture = paragraph.pictures.get(placed.id as usize);
                        if let Some(picture) = picture {
                            self.picture(&picture.pixels, x + placed.x, y + placed.y);
                        }
                        continue;
                    }
                };
                let run = glyphs.run();
                // A glyph run is the part of a shaped run in one style: it
                // skips the glyphs that earlier glyph runs of this shaped run
                // took on the line, and takes the next `wanted`.
                let wanted = glyphs.positioned_glyphs().count();
                let skip = if taken.0 == run.index() { taken.1 } else { 0 };
                taken = (run.index(), skip + wanted);
                let clusters = clusters_after(run, skip, wanted);
                // A subscript or superscript sits off the baseline, each
                // glyph by its own text: a glyph run may hold both.
                let drops: Vec<f32> = (clusters.iter())
                    .flat_map(|cluster| {
                        let range = cluster.text_range();
                        let drop = paragraph.drop_at(&(start + range.start..start + range.end));
                        cluster.glyphs().map(move |_| drop)
                    })
                    .collect();
                let drop_of = |at: usize| drops.get(at).copied().unwrap_or(0.0);
                let font = run.font();
                let Ok(face) = FontRef::from_index(font.data.data(), font.index) else {
                    continue;
                };
                let coords: Vec<NormalizedCoord> = (run.normalized_coords().iter())
                    .map(|coord| F2Dot14::from_bits(coord.to_bits()))
                    .collect();
                // An italic the font has no face for is slanted.
                let skew =
                    (run.synthesis().skew()).map_or(0.0, |degrees| degrees.to_radians().tan());
                let color = over
                    .and_then(|(color, _)| color)
                    .unwrap_or(glyphs.style().brush);
                let outlines = face.outline_glyphs();
                let font_size = run.font_size();
                let size = Size::new(font_size);
                let clip = over.map(|(_, mask)| mask);
                let variation = coords.iter().map(|coord| coord.to_bits());
                let key = Face::new(font.data.id(), font.index, font_size, skew, variation);
                for (at, glyph) in glyphs.positioned_glyphs().enumerate() {
                    let (gx, gy) = (x + glyph.x, y + drop_of(at) + glyph.y);
                    // The glyph, its outline's origin `dx`, `dy` into its pixel.
                    let make = |(dx, dy): (f32, f32)| {
                        let mut path = PathBuilder::new();
                        let mut pen = Pen {
                            path: &mut path,
                            x: dx,
                            y: dy,
                            skew,
                        };
                        // A glyph that cannot be drawn is left out.
                        let settings = DrawSettings::unhinted(size, LocationRef::new(&coords));
                        if let Some(outline) = outlines.get(GlyphId::new(glyph.id)) {
                            let _ = outline.draw(settings, &mut pen);
                        }
                        let colour = face.color_glyphs().get(GlyphId::new(glyph.id)).is_some();
                        Glyph::new(colour, path.finish())
                    };
                    let colour = |pixmap: &mut Pixmap| {
                        colr::draw(pixmap, &face, glyph.id, font_size, (gx, gy), color)
                    };
                    self.glyphs.draw(
                        &mut self.pixmap,
                        (key, glyph.id),
                        Origin::of(gx, gy),
                        (color, clip),
                        make,
                        colour,
                    );
                }
                if glyphs.style().strikethrough.is_some() {
                    let metrics = run.font_metrics();
                    let thickness = metrics.strikethrough_size.max(1.0);
                    // One line for each stretch of equal drop.
                    let stretches = stretches(paragraph, start, &clusters);
                    let mut left = x + glyphs.offset();
                    for (drop, width) in stretches {
                        let top = y + drop + glyphs.baseline() - metrics.strikethrough_offset;
                        let mask = over.map(|(_, mask)| mask);
                        self.rect_masked((left, top, width, thickness), color, mask);
                        left += width;
                    }
                }
            }
        }
    }

    /// Draws `pixels` with its top left at (`x`, `y`) rounded to whole
    /// pixels, as far as it reaches the canvas.
    pub fn picture(&mut self, pixels: &Pixels, x: f32, y: f32) {
        let Some(pixmap) = pixels.map() else {
            return;
        };
        let (left, top) = (x.round() as i64, y.round() as i64);
        let (width, height) = (i64::from(pixels.width()), i64::from(pixels.height()));
        // Wholly off the canvas: nothing to draw.
        if top >= i64::from(self.pixmap.height())
            || top + height <= 0
            || left >= i64::from(self.pixmap.width())
            || left + width <= 0
        {
            return;
        }
        self.pixmap.draw_pixmap(
            left as i32,
            top as i32,
            pixmap,
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    /// Draws the outline of a rectangle, `thickness` thick, inside it.
    pub fn frame(&mut self, x: f32, y: f32, width: f32, height: f32, thickness: f32, color: Brush) {
        self.rect(x, y, width, thickness, color);
        self.rect(x, y + height - thickness, width, thickness, color);
        self.rect(x, y, thickness, height, color);
        self.rect(x + width - thickness, y, thickness, height, color);
    }

    /// Fills a rectangle.
    pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Brush) {
        if let Some(rect) = Rect::from_xywh(x, y, width, height) {
            (self.pixmap).fill_rect(rect, &paint(color, 255), Transform::identity(), None);
        }
    }

    /// Fills a rectangle, `(x, y, width, height)`, where `mask` lets it.
    fn rect_masked(
        &mut self,
        (x, y, width, height): (f32, f32, f32, f32),
        color: Brush,
        mask: Option<&tiny_skia::Mask>,
    ) {
        if let Some(rect) = Rect::from_xywh(x, y, width, height) {
            (self.pixmap).fill_rect(rect, &paint(color, 255), Transform::identity(), mask);
        }
    }

    /// Fills a rectangle with corners rounded to `radius`.
    pub fn round_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: Brush,
    ) {
        self.round_rect_alpha(x, y, width, height, radius, color, 255);
    }

    /// Fills a rectangle with corners rounded to `radius`, in `color` at
    /// the opacity `alpha`.
    #[allow(clippy::too_many_arguments)]
    pub fn round_rect_alpha(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: Brush,
        alpha: u8,
    ) {
        let radii = [radius; 4];
        if let Some(path) = rounded_path(x, y, width, height, radii) {
            self.pixmap.fill_path(
                &path,
                &paint(color, alpha),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    /// Fills a rectangle with its corners rounded to `radii`: top left,
    /// top right, bottom right, bottom left.
    pub fn corners_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radii: [f32; 4],
        color: Brush,
    ) {
        if let Some(path) = rounded_path(x, y, width, height, radii) {
            self.pixmap.fill_path(
                &path,
                &paint(color, 255),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    /// Draws the outline of a rectangle with corners rounded to `radius`,
    /// one pixel thick, inside it.
    pub fn round_frame(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: Brush,
    ) {
        let outer = rounded_path(x, y, width, height, [radius; 4]);
        let inner = rounded_path(
            x + 1.0,
            y + 1.0,
            width - 2.0,
            height - 2.0,
            [(radius - 1.0).max(0.0); 4],
        );
        let (Some(outer), Some(inner)) = (outer, inner) else {
            return;
        };
        let mut path = PathBuilder::new();
        path.push_path(&outer);
        path.push_path(&inner);
        if let Some(path) = path.finish() {
            self.pixmap.fill_path(
                &path,
                &paint(color, 255),
                FillRule::EvenOdd,
                Transform::identity(),
                None,
            );
        }
    }

    /// Fades out what is drawn in the `width` pixels from `x`, from the
    /// top down to `height`: towards the right edge, or towards the left
    /// one when `left`, where it is almost gone.
    pub fn fade(&mut self, x: f32, width: f32, height: f32, left: bool) {
        let (Some(rect), true) = (Rect::from_xywh(x, 0.0, width, height), width > 0.0) else {
            return;
        };
        let (clear, gone) = (
            Color::from_rgba8(0, 0, 0, 0),
            Color::from_rgba8(0, 0, 0, 230),
        );
        let stops = match left {
            true => vec![GradientStop::new(0.0, gone), GradientStop::new(1.0, clear)],
            false => vec![GradientStop::new(0.0, clear), GradientStop::new(1.0, gone)],
        };
        let shader = LinearGradient::new(
            Point::from_xy(x, 0.0),
            Point::from_xy(x + width, 0.0),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        );
        let Some(shader) = shader else {
            return;
        };
        let paint = Paint {
            shader,
            blend_mode: BlendMode::DestinationOut,
            ..Paint::default()
        };
        self.pixmap
            .fill_rect(rect, &paint, Transform::identity(), None);
    }

    /// Draws on a transparent canvas `width` by `height` of its own with
    /// `draw`, which shares this one's glyphs, and lays it over this one
    /// with its top left at (`x`, `y`).
    pub fn layer(
        &mut self,
        (width, height): (u32, u32),
        (x, y): (f32, f32),
        draw: impl FnOnce(&mut Canvas),
    ) {
        let mut layer = Canvas::new(width, height, &mut *self.glyphs);
        draw(&mut layer);
        self.pixmap.draw_pixmap(
            x.round() as i32,
            y.round() as i32,
            layer.pixmap.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    /// The pixels drawn, not premultiplied, as images are sent.
    pub fn into_image(self) -> RgbaImage {
        let (width, height) = (self.pixmap.width(), self.pixmap.height());
        let mut pixels = self.pixmap.take();
        demultiply_in_place(&mut pixels);
        RgbaImage::from_raw(width, height, pixels).expect("as many pixels as the canvas has")
    }
}

/// `pixel`, red, green, blue and alpha, with its colours multiplied by its
/// alpha, as tiny-skia holds pixels.
pub(super) fn premultiplied([red, green, blue, alpha]: [u8; 4]) -> [u8; 4] {
    let scale = |channel: u8| ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8;
    [scale(red), scale(green), scale(blue), alpha]
}

/// Demultiplies premultiplied RGBA `bytes` in place, the same as
/// `Pixmap::take_demultiplied`, but two pixels at a time. Most of a canvas is
/// clear or opaque, and neither needs the division: a clear pixel demultiplies
/// to zero, and an opaque one to itself. Only a pair with a translucent pixel
/// in it goes through `demultiply`.
fn demultiply_in_place(bytes: &mut [u8]) {
    let (pairs, rest) = bytes.as_chunks_mut::<8>();
    for pair in pairs {
        let clear = pair.iter().all(|&byte| byte == 0);
        let opaque = pair[3] == 255 && pair[7] == 255;
        if !clear && !opaque {
            for pixel in pair.as_chunks_mut::<4>().0 {
                demultiply_pixel(pixel);
            }
        }
    }
    for pixel in rest.as_chunks_mut::<4>().0 {
        demultiply_pixel(pixel);
    }
}

/// One premultiplied RGBA pixel, demultiplied in place. The canvas holds
/// only colours no brighter than their alpha; one that was would be held to
/// its alpha, so that a canvas never panics over it.
fn demultiply_pixel(pixel: &mut [u8; 4]) {
    let alpha = pixel[3];
    if alpha == 255 {
        return;
    }
    let color = PremultipliedColorU8::from_rgba(
        pixel[0].min(alpha),
        pixel[1].min(alpha),
        pixel[2].min(alpha),
        alpha,
    )
    .expect("colours held to the alpha are at most the alpha")
    .demultiply();
    *pixel = [color.red(), color.green(), color.blue(), color.alpha()];
}

/// A rectangle's outline with its corners rounded to `radii`: top left,
/// top right, bottom right, bottom left.
fn rounded_path(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radii: [f32; 4],
) -> Option<tiny_skia::Path> {
    let limit = (width / 2.0).min(height / 2.0).max(0.0);
    let [tl, tr, br, bl] = radii.map(|radius| radius.min(limit));
    // How far a cubic's control point lies along the tangent, for a
    // quarter circle.
    let k = 0.5523;
    let (right, bottom) = (x + width, y + height);
    let mut path = PathBuilder::new();
    path.move_to(x + tl, y);
    path.line_to(right - tr, y);
    path.cubic_to(
        right - tr + k * tr,
        y,
        right,
        y + tr - k * tr,
        right,
        y + tr,
    );
    path.line_to(right, bottom - br);
    path.cubic_to(
        right,
        bottom - br + k * br,
        right - br + k * br,
        bottom,
        right - br,
        bottom,
    );
    path.line_to(x + bl, bottom);
    path.cubic_to(
        x + bl - k * bl,
        bottom,
        x,
        bottom - bl + k * bl,
        x,
        bottom - bl,
    );
    path.line_to(x, y + tl);
    path.cubic_to(x, y + tl - k * tl, x + tl - k * tl, y, x + tl, y);
    path.close();
    path.finish()
}

fn paint(color: Brush, alpha: u8) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(color[0], color[1], color[2], alpha));
    paint.anti_alias = true;
    paint
}

/// Turns a glyph's outline, in pixels up from its origin, into a path on
/// the canvas, slanted by `skew`.
struct Pen<'a> {
    path: &'a mut PathBuilder,
    x: f32,
    y: f32,
    skew: f32,
}

impl Pen<'_> {
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + x + self.skew * y, self.y - y)
    }
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.path.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.path.line_to(x, y);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (cx, cy) = self.point(cx, cy);
        let (x, y) = self.point(x, y);
        self.path.quad_to(cx, cy, x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (cx0, cy0) = self.point(cx0, cy0);
        let (cx1, cy1) = self.point(cx1, cy1);
        let (x, y) = self.point(x, y);
        self.path.cubic_to(cx0, cy0, cx1, cy1, x, y);
    }

    fn close(&mut self) {
        self.path.close();
    }
}

/// The stretches of `clusters` with the same drop, each with its drop and
/// its width.
fn stretches<B: parley::Brush>(
    paragraph: &Paragraph,
    start: usize,
    clusters: &[parley::Cluster<'_, B>],
) -> Vec<(f32, f32)> {
    let mut stretches: Vec<(f32, f32)> = Vec::new();
    for cluster in clusters {
        let range = cluster.text_range();
        let drop = paragraph.drop_at(&(start + range.start..start + range.end));
        match stretches.last_mut() {
            Some((last, width)) if *last == drop => *width += cluster.advance(),
            _ => stretches.push((drop, cluster.advance())),
        }
    }
    stretches
}

/// The clusters of `run` that hold the `wanted` glyphs after its first `skip`.
fn clusters_after<'a, B: parley::Brush>(
    run: &parley::Run<'a, B>,
    skip: usize,
    wanted: usize,
) -> Vec<parley::Cluster<'a, B>> {
    let mut passed = 0;
    let mut clusters = Vec::new();
    for cluster in run.visual_clusters() {
        if passed >= skip && passed < skip + wanted {
            clusters.push(cluster);
        }
        passed += cluster.glyphs().count();
    }
    clusters
}

#[cfg(test)]
mod tests;
