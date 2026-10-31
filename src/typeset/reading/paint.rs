//! Laid-out text drawn as pixels: its glyphs' outlines, through skrifa,
//! filled with tiny-skia, on the chips of its inline code, and the rules
//! and bars around blocks. What is not drawn stays transparent, so the
//! page shows through.

use image::RgbaImage;
use resvg::tiny_skia::{
    self, BlendMode, Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap,
    PixmapPaint, Point, Rect, SpreadMode, Transform,
};
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::types::F2Dot14;
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::typeset::reading::colr;
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

/// A picture being drawn, transparent where nothing is.
pub struct Canvas {
    pixmap: Pixmap,
}

impl Canvas {
    /// A transparent canvas `width` by `height` pixels, at least one each.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            pixmap: Pixmap::new(width.max(1), height.max(1)).expect("a size of at least 1"),
        }
    }

    /// A canvas `width` by `height` pixels in `background`, or transparent
    /// where there is none.
    pub fn on(width: u32, height: u32, background: Option<Brush>) -> Self {
        let mut canvas = Self::new(width, height);
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
        for line in paragraph.layout.lines() {
            let metrics = line.metrics();
            if y + metrics.block_max_coord + reach < 0.0
                || y + metrics.block_min_coord - reach > height
            {
                continue;
            }
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
                // A subscript or superscript sits off the baseline, each
                // glyph by its own text: a glyph run may hold both.
                let drops: Vec<f32> = (run.visual_clusters())
                    .flat_map(|cluster| {
                        let drop = paragraph.drop_at(&cluster.text_range());
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
                let mut path = PathBuilder::new();
                let outlines = face.outline_glyphs();
                let size = Size::new(run.font_size());
                for (at, glyph) in glyphs.positioned_glyphs().enumerate() {
                    let y = y + drop_of(at);
                    let origin = (x + glyph.x, y + glyph.y);
                    if colr::draw(
                        &mut self.pixmap,
                        &face,
                        glyph.id,
                        run.font_size(),
                        origin,
                        color,
                    ) {
                        continue;
                    }
                    let Some(outline) = outlines.get(GlyphId::new(glyph.id)) else {
                        continue;
                    };
                    let mut pen = Pen {
                        path: &mut path,
                        x: x + glyph.x,
                        y: y + glyph.y,
                        skew,
                    };
                    // A glyph that cannot be drawn is left out.
                    let settings = DrawSettings::unhinted(size, LocationRef::new(&coords));
                    let _ = outline.draw(settings, &mut pen);
                }
                if let Some(path) = path.finish() {
                    self.pixmap.fill_path(
                        &path,
                        &paint(color, 255),
                        FillRule::Winding,
                        Transform::identity(),
                        over.map(|(_, mask)| mask),
                    );
                }
                if glyphs.style().strikethrough.is_some() {
                    let metrics = run.font_metrics();
                    let thickness = metrics.strikethrough_size.max(1.0);
                    let top = y + drop_of(0) + glyphs.baseline() - metrics.strikethrough_offset;
                    self.rect(x + glyphs.offset(), top, glyphs.advance(), thickness, color);
                }
            }
        }
    }

    /// Draws `image` with its top left at (`x`, `y`) rounded to whole
    /// pixels, as far as it reaches the canvas.
    pub fn picture(&mut self, image: &RgbaImage, x: f32, y: f32) {
        let (left, top) = (x.round() as i64, y.round() as i64);
        let (width, height) = (i64::from(image.width()), i64::from(image.height()));
        // The rows of the image that are on the canvas.
        let from = (-top).clamp(0, height);
        let to = (i64::from(self.pixmap.height()) - top).clamp(0, height);
        if from >= to || left >= i64::from(self.pixmap.width()) || left + width <= 0 {
            return;
        }
        let data: Vec<u8> = (image.rows().skip(from as usize).take((to - from) as usize))
            .flat_map(|row| row.flat_map(|pixel| premultiplied(pixel.0)))
            .collect();
        let Some(size) = tiny_skia::IntSize::from_wh(width as u32, (to - from) as u32) else {
            return;
        };
        let Some(pixmap) = Pixmap::from_vec(data, size) else {
            return;
        };
        self.pixmap.draw_pixmap(
            left as i32,
            (top + from) as i32,
            pixmap.as_ref(),
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

    /// Draws `other` over this canvas with its top left at (`x`, `y`).
    pub fn overlay(&mut self, other: &Canvas, x: f32, y: f32) {
        self.pixmap.draw_pixmap(
            x.round() as i32,
            y.round() as i32,
            other.pixmap.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    /// The pixels drawn, not premultiplied, as images are sent.
    pub fn into_image(self) -> RgbaImage {
        let (width, height) = (self.pixmap.width(), self.pixmap.height());
        let pixels = (self.pixmap.pixels().iter())
            .flat_map(|pixel| {
                let color = pixel.demultiply();
                [color.red(), color.green(), color.blue(), color.alpha()]
            })
            .collect();
        RgbaImage::from_raw(width, height, pixels).expect("as many pixels as the canvas has")
    }
}

/// `pixel`, red, green, blue and alpha, with its colours multiplied by its
/// alpha, as tiny-skia holds pixels.
fn premultiplied([red, green, blue, alpha]: [u8; 4]) -> [u8; 4] {
    let scale = |channel: u8| ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8;
    [scale(red), scale(green), scale(blue), alpha]
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

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::typeset::reading::Fonts;
    use crate::typeset::reading::text::{Look, Run, Script, lay_out};

    const INK: Brush = [0x20, 0xc0, 0x40];
    const CHIP: Brush = [0x40, 0x40, 0xa0];

    const LOOK: Look = Look {
        color: INK,
        ..Look::PLAIN
    };

    fn drawn(runs: &[Run], chip: Option<Brush>) -> RgbaImage {
        let paragraph = lay_out(&Fonts::fixture(), runs, &LOOK, 400.0);
        let mut canvas = Canvas::new(420, 30);
        canvas.text(&paragraph, 10.0, 3.0, chip);
        canvas.into_image()
    }

    fn count(image: &RgbaImage, color: Brush) -> usize {
        (image.pixels())
            .filter(|pixel| pixel.0 == [color[0], color[1], color[2], 255])
            .count()
    }

    #[test]
    fn text_is_drawn_in_its_colour_on_transparent_pixels() {
        let image = drawn(&[Run::new("Reading mode")], None);
        assert!(
            count(&image, INK) > 100,
            "{} pixels of ink",
            count(&image, INK)
        );
        assert_eq!(image.get_pixel(0, 0).0, [0; 4]);
        // Nothing left of where the text starts.
        assert!((0..30).all(|y| image.get_pixel(9, y).0[3] == 0));
    }

    #[test]
    fn inline_code_is_drawn_on_a_chip() {
        let runs = [
            Run::new("Run "),
            Run {
                code: true,
                ..Run::new("make check")
            },
        ];
        let without = drawn(&runs, None);
        let with = drawn(&runs, Some(CHIP));
        assert_eq!(count(&without, CHIP), 0);
        assert!(
            count(&with, CHIP) > 500,
            "{} pixels of chip",
            count(&with, CHIP)
        );
        // The text over the chip, not under it.
        assert!(
            count(&with, INK) > 50,
            "{} pixels of ink",
            count(&with, INK)
        );
        // Text without code gets no chip.
        assert_eq!(count(&drawn(&[Run::new("Run")], Some(CHIP)), CHIP), 0);
    }

    /// The first and last row of `image` that have ink.
    fn ink_rows(image: &RgbaImage) -> (u32, u32) {
        let rows: Vec<u32> = (0..image.height())
            .filter(|&y| {
                (0..image.width()).any(|x| image.get_pixel(x, y).0 == [0x20, 0xc0, 0x40, 255])
            })
            .collect();
        (rows[0], rows[rows.len() - 1])
    }

    #[test]
    fn marked_text_is_drawn_on_its_background() {
        let marked = Run {
            background: Some(CHIP),
            ..Run::new("found")
        };
        let image = drawn(&[Run::new("a "), marked], None);
        assert!(
            count(&image, CHIP) > 300,
            "{} pixels of background",
            count(&image, CHIP)
        );
        // The text over the box, not under it.
        assert!(count(&image, INK) > 50);
        assert_eq!(count(&drawn(&[Run::new("a found")], None), CHIP), 0);
    }

    #[test]
    fn a_subscript_is_lowered_and_a_superscript_raised_and_both_are_smaller() {
        let script = |drop| Run {
            script: Some(Script { size: 0.8, drop }),
            ..Run::new("xx")
        };
        let (top, bottom) = ink_rows(&drawn(&[Run::new("xx")], None));
        let (sub_top, sub_bottom) = ink_rows(&drawn(&[script(0.2)], None));
        let (sup_top, sup_bottom) = ink_rows(&drawn(&[script(-0.33)], None));
        assert!(
            sub_bottom > bottom,
            "sub ends at {sub_bottom}, text at {bottom}"
        );
        assert!(
            sup_bottom < bottom,
            "sup ends at {sup_bottom}, text at {bottom}"
        );
        assert!(sup_top < top, "sup starts at {sup_top}, text at {top}");
        assert!(
            sub_bottom - sub_top < bottom - top,
            "sub is {} high, text {}",
            sub_bottom - sub_top,
            bottom - top
        );
    }

    /// The last row of `image` with ink in the columns `from..to`.
    fn ink_bottom_in(image: &RgbaImage, from: u32, to: u32) -> u32 {
        (0..image.height())
            .rev()
            .find(|&y| (from..to).any(|x| image.get_pixel(x, y).0 == [0x20, 0xc0, 0x40, 255]))
            .expect("ink in the columns")
    }

    #[test]
    fn a_superscript_beside_a_subscript_keeps_its_own_shift() {
        let script = |text: &str, drop| Run {
            script: Some(Script { size: 0.8, drop }),
            ..Run::new(text)
        };
        let sup = drawn(&[Run::new("x"), script("ab", -0.33)], None);
        let edge = (0..sup.width())
            .rev()
            .find(|&x| (0..sup.height()).any(|y| sup.get_pixel(x, y).0[3] > 0))
            .unwrap()
            + 2;
        let both = drawn(
            &[
                Run::new("x"),
                script("ab", -0.33),
                script("cd", 0.2),
                Run::new("x"),
            ],
            None,
        );
        let (high, low) = (
            ink_bottom_in(&both, 12, edge),
            ink_bottom_in(&both, edge, both.width()),
        );
        assert!(low > high, "sub ends at {low}, sup beside it at {high}");
    }

    #[test]
    fn italic_text_is_slanted() {
        let upright = drawn(&[Run::new("llll")], None);
        let italic = drawn(
            &[Run {
                italic: true,
                ..Run::new("llll")
            }],
            None,
        );
        assert!(upright != italic, "italic text drawn upright");
    }

    #[test]
    fn emoji_are_drawn_in_their_colours() {
        let image = drawn(&[Run::new("Kiri 😀")], None);
        let coloured = (image.pixels())
            .filter(|pixel| pixel.0[3] == 255 && pixel.0[..3] != INK)
            .count();
        assert!(coloured > 100, "{coloured} pixels in colour");
        let yellow = (image.pixels())
            .filter(|pixel| pixel.0[0] > 200 && pixel.0[1] > 150 && pixel.0[2] < 100)
            .count();
        assert!(yellow > 50, "{yellow} yellow pixels");
    }

    fn red(width: u32, height: u32, alpha: u8) -> RgbaImage {
        RgbaImage::from_pixel(width, height, image::Rgba([200, 0, 0, alpha]))
    }

    #[test]
    fn a_picture_is_drawn_where_it_is_and_cut_at_the_canvas() {
        let mut canvas = Canvas::new(10, 10);
        canvas.picture(&red(4, 4, 255), 2.0, 3.0);
        let image = canvas.into_image();
        assert_eq!(image.get_pixel(2, 3).0, [200, 0, 0, 255]);
        assert_eq!(image.get_pixel(5, 6).0, [200, 0, 0, 255]);
        assert_eq!(image.get_pixel(6, 6).0[3], 0);
        assert_eq!(image.get_pixel(1, 3).0[3], 0);
        assert_eq!(image.get_pixel(2, 2).0[3], 0);
        // Reaching past the top, the left and the bottom edges.
        let mut canvas = Canvas::new(10, 10);
        canvas.picture(&red(4, 4, 255), -2.0, -3.0);
        canvas.picture(&red(4, 4, 255), 8.0, 8.0);
        let image = canvas.into_image();
        assert_eq!(image.get_pixel(0, 0).0, [200, 0, 0, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [200, 0, 0, 255]);
        assert_eq!(image.get_pixel(2, 0).0[3], 0);
        assert_eq!(image.get_pixel(0, 1).0[3], 0);
        assert_eq!(image.get_pixel(9, 9).0, [200, 0, 0, 255]);
        // Wholly off the canvas: nothing, and no trouble.
        let mut canvas = Canvas::new(10, 10);
        canvas.picture(&red(4, 4, 255), 20.0, 0.0);
        canvas.picture(&red(4, 4, 255), 0.0, -20.0);
        canvas.picture(&red(4, 4, 255), -20.0, 0.0);
        assert!(canvas.into_image().pixels().all(|pixel| pixel.0[3] == 0));
    }

    #[test]
    fn a_mark_is_a_box_with_the_text_drawn_again_in_it() {
        let paragraph = lay_out(&Fonts::fixture(), &[Run::new("Ab cd")], &LOOK, 400.0);
        let lines = paragraph.lines();
        let boxes = paragraph.boxes(3..5);
        let [part] = boxes[..] else {
            panic!("one box: {boxes:?}");
        };
        let mark = TextMark {
            range: 3..5,
            foreground: Some([0, 200, 0]),
            background: Some([200, 0, 200]),
        };
        let mut canvas = Canvas::new(120, 40);
        canvas.text_marked(&paragraph, 4.0, 5.0, None, &[mark]);
        let image = canvas.into_image();
        let (left, right) = (4.0 + part.left, 4.0 + part.right);
        let (top, bottom) = (5.0 + lines[0].top, 5.0 + lines[0].top + lines[0].height);
        let count = |from: f32, to: f32, colour: [u8; 3]| {
            (from.round() as u32..to.round() as u32)
                .flat_map(|x| (top.round() as u32..bottom.round() as u32).map(move |y| (x, y)))
                .filter(|&(x, y)| {
                    let pixel = image.get_pixel(x, y).0;
                    pixel[3] > 100 && (0..3).all(|i| pixel[i].abs_diff(colour[i]) <= 3)
                })
                .count()
        };
        // The box is in the background colour, with the text in the
        // foreground colour over it, and none of the text's own colour.
        assert!(count(left, right, [200, 0, 200]) > 100);
        let greenish = (left.round() as u32..right.round() as u32)
            .flat_map(|x| (top.round() as u32..bottom.round() as u32).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                let [red, green, ..] = image.get_pixel(x, y).0;
                green > red.saturating_add(40)
            })
            .count();
        assert!(greenish > 10, "{greenish}");
        assert_eq!(count(left, right, INK), 0);
        // Beside it the text is as it was, with no box.
        assert!(
            count(4.0, left - 2.0, INK) > 5,
            "{}",
            count(4.0, left - 2.0, INK)
        );
        assert_eq!(count(4.0, left - 2.0, [200, 0, 200]), 0);
        assert_eq!(count(right + 2.0, 100.0, [200, 0, 200]), 0);
    }

    #[test]
    fn marks_of_one_colour_and_of_another_in_a_paragraph_are_each_drawn_in_theirs() {
        let paragraph = lay_out(&Fonts::fixture(), &[Run::new("ab cd ef gh")], &LOOK, 400.0);
        let green = Some([0, 200, 0]);
        let marks = [
            TextMark {
                range: 0..2,
                foreground: green,
                background: Some([200, 0, 200]),
            },
            TextMark {
                range: 3..5,
                foreground: Some([250, 250, 0]),
                background: Some([0, 0, 90]),
            },
            TextMark {
                range: 6..8,
                foreground: green,
                background: Some([200, 0, 200]),
            },
        ];
        let mut canvas = Canvas::new(160, 40);
        canvas.text_marked(&paragraph, 4.0, 5.0, None, &marks);
        let image = canvas.into_image();
        let lines = paragraph.lines();
        let (top, bottom) = (5.0 + lines[0].top, 5.0 + lines[0].top + lines[0].height);
        let within = |range: std::ops::Range<usize>, test: &dyn Fn([u8; 4]) -> bool| {
            let part = paragraph.boxes(range)[0];
            let (left, right) = (
                (4.0 + part.left).round() as u32,
                (4.0 + part.right).round() as u32,
            );
            (left..right)
                .flat_map(|x| (top.round() as u32..bottom.round() as u32).map(move |y| (x, y)))
                .filter(|&(x, y)| test(image.get_pixel(x, y).0))
                .count()
        };
        let greenish =
            |p: [u8; 4]| p[1] > p[0].saturating_add(40) && p[1] > p[2].saturating_add(40);
        let yellowish = |p: [u8; 4]| p[0] > 150 && p[1] > 150 && p[2] < 100;
        // The first and third in green, the second in yellow, none in the
        // text's own colour.
        assert!(within(0..2, &greenish) > 5 && within(6..8, &greenish) > 5);
        assert!(within(3..5, &yellowish) > 5 && within(3..5, &greenish) == 0);
        assert!(within(0..2, &yellowish) == 0 && within(6..8, &yellowish) == 0);
    }

    #[test]
    fn a_translucent_picture_keeps_its_colour_and_its_alpha() {
        let mut canvas = Canvas::new(4, 4);
        canvas.picture(&red(2, 2, 128), 0.0, 0.0);
        let pixel = canvas.into_image().get_pixel(0, 0).0;
        assert_eq!(pixel[3], 128);
        assert!(pixel[0].abs_diff(200) <= 2, "{pixel:?}");
    }

    #[test]
    fn a_picture_in_a_line_is_drawn_where_the_line_puts_it() {
        let red = Run::picture(
            crate::typeset::reading::text::InlinePicture {
                pixels: Rc::new(red(10, 10, 255)),
                id: 1,
                depth: 0.0,
            },
            None,
        );
        let paragraph = lay_out(&Fonts::fixture(), &[Run::new("Ab "), red], &LOOK, 400.0);
        let mut canvas = Canvas::new(120, 40);
        canvas.text(&paragraph, 0.0, 5.0, None);
        let image = canvas.into_image();
        let reds = image
            .pixels()
            .filter(|pixel| pixel.0 == [200, 0, 0, 255])
            .count();
        assert_eq!(reds, 100);
        // After the text, on its baseline: the red square's bottom row is
        // where the glyphs' feet are.
        let (x, y) = (0..120)
            .flat_map(|x| (0..40).map(move |y| (x, y)))
            .find(|&(x, y)| image.get_pixel(x, y).0 == [200, 0, 0, 255])
            .expect("a red pixel");
        assert!(x > 20, "{x}");
        let baseline = paragraph.first_baseline() + 5.0;
        assert_eq!(y + 10, baseline.round() as u32);
    }

    /// The rows of `image` that have ink, first and last.
    fn inked_rows(image: &RgbaImage) -> Option<(u32, u32)> {
        let rows: Vec<u32> = (0..image.height())
            .filter(|&y| (0..image.width()).any(|x| image.get_pixel(x, y).0[3] > 0))
            .collect();
        Some((*rows.first()?, *rows.last()?))
    }

    #[test]
    fn text_reaching_only_a_pixel_or_two_into_the_canvas_still_draws_there() {
        // Lines of half the font size, so that glyphs reach well past
        // their line boxes: a skip that went by the boxes alone, or by too
        // little room around them, would leave these out.
        let look = Look {
            line_height: 0.5,
            ..LOOK
        };
        let paragraph = lay_out(&Fonts::fixture(), &[Run::new("Hxgpq")], &look, 400.0);
        let draw = |height: u32, y: f32| {
            let mut canvas = Canvas::new(120, height);
            canvas.text(&paragraph, 0.0, y, None);
            canvas.into_image()
        };
        // The whole text on a canvas it is well inside of, and where its
        // ink is from the text's own top.
        let whole = draw(300, 100.0);
        let (top, bottom) = inked_rows(&whole).expect("ink");
        let (top, bottom) = (top as f32 - 100.0, bottom as f32 - 100.0);
        // The same text on a canvas of 30 rows, moved so that the ink's
        // last row is the canvas's third, and its first row the
        // second last.
        for (y, row) in [(2.0 - bottom, 2), (28.0 - top, 28)] {
            let small = draw(30, y);
            let (first, last) = inked_rows(&small).expect("ink at the edge");
            assert!(
                first <= row && row <= last,
                "{first}..{last} for ink at row {row}"
            );
            // The same pixels as those rows of the whole text, but for the
            // canvas's first and last rows, where a glyph cut by the edge
            // may antialias a level or two differently.
            let from = (100.0 - y) as u32 + 1;
            let inner = |image: &RgbaImage, from: u32| {
                image::imageops::crop_imm(image, 0, from, 120, 28).to_image()
            };
            let (small, cut) = (inner(&small, 1), inner(&whole, from));
            let far = (small.pixels().zip(cut.pixels()))
                .filter(|(a, b)| a.0.iter().zip(&b.0).any(|(a, b)| a.abs_diff(*b) > 8))
                .count();
            assert_eq!(far, 0, "{far} pixels differ at y = {y}");
        }
        // Far from the canvas, nothing draws.
        assert_eq!(inked_rows(&draw(30, -2000.0)), None);
        assert_eq!(inked_rows(&draw(30, 2000.0)), None);
    }

    #[test]
    fn rectangles_fill_their_pixels() {
        let mut canvas = Canvas::new(10, 10);
        canvas.rect(2.0, 3.0, 4.0, 1.0, INK);
        let image = canvas.into_image();
        assert_eq!(image.get_pixel(2, 3).0, [INK[0], INK[1], INK[2], 255]);
        assert_eq!(image.get_pixel(2, 4).0, [0; 4]);
        assert_eq!(count(&image, INK), 4);
    }
}
