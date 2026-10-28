//! Laid-out text drawn as pixels: its glyphs' outlines, through skrifa,
//! filled with tiny-skia, on the chips of its inline code, and the rules
//! and bars around blocks. What is not drawn stays transparent, so the
//! page shows through.

use image::RgbaImage;
use resvg::tiny_skia::{
    BlendMode, Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap,
    PixmapPaint, Point, Rect, SpreadMode, Transform,
};
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::types::F2Dot14;
use skrifa::{FontRef, GlyphId, MetadataProvider};

use super::colr;
use super::text::{Brush, Paragraph};
use parley::PositionedLayoutItem;

/// How far inline code's chip reaches above the baseline, how tall it is
/// and how far it reaches past the code on each side, in font sizes of the
/// text around it; and the radius of its corners. It sits a little lower
/// than the line, as the code is smaller than its text.
const CHIP_ABOVE: f32 = 0.95;
const CHIP_HEIGHT: f32 = 1.25;
const CHIP_PAD: f32 = 0.12;
const CHIP_RADIUS: f32 = 0.25;

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

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// Draws `paragraph` with the top left of its text at (`x`, `y`), on
    /// chips of `chip` behind its inline code when given.
    pub fn text(&mut self, paragraph: &Paragraph, x: f32, y: f32, chip: Option<Brush>) {
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
        for line in paragraph.layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyphs) = item else {
                    continue;
                };
                let run = glyphs.run();
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
                let color = glyphs.style().brush;
                let mut path = PathBuilder::new();
                let outlines = face.outline_glyphs();
                let size = Size::new(run.font_size());
                for glyph in glyphs.positioned_glyphs() {
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
                        None,
                    );
                }
                if glyphs.style().strikethrough.is_some() {
                    let metrics = run.font_metrics();
                    let thickness = metrics.strikethrough_size.max(1.0);
                    let top = y + glyphs.baseline() - metrics.strikethrough_offset;
                    self.rect(x + glyphs.offset(), top, glyphs.advance(), thickness, color);
                }
            }
        }
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
        let r = radius.min(width / 2.0).min(height / 2.0);
        // How far a cubic's control point lies along the tangent, for a
        // quarter circle.
        let k = 0.5523 * r;
        let (right, bottom) = (x + width, y + height);
        let mut path = PathBuilder::new();
        path.move_to(x + r, y);
        path.line_to(right - r, y);
        path.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
        path.line_to(right, bottom - r);
        path.cubic_to(
            right,
            bottom - r + k,
            right - r + k,
            bottom,
            right - r,
            bottom,
        );
        path.line_to(x + r, bottom);
        path.cubic_to(x + r - k, bottom, x, bottom - r + k, x, bottom - r);
        path.line_to(x, y + r);
        path.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
        path.close();
        if let Some(path) = path.finish() {
            self.pixmap.fill_path(
                &path,
                &paint(color, alpha),
                FillRule::Winding,
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
    use super::*;
    use crate::reading::Fonts;
    use crate::reading::text::{Look, Run, lay_out};

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
