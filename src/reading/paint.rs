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
        self.glyphs(paragraph, x, y, None);
        for mark in marks {
            self.mark(paragraph, x, y, mark);
        }
    }

    /// Lays `mark` over the text drawn at (`x`, `y`): a box of its
    /// background colour on each line it covers, and the glyphs in it
    /// again in its foreground colour.
    fn mark(&mut self, paragraph: &Paragraph, x: f32, y: f32, mark: &TextMark) {
        let lines = paragraph.lines();
        let mut mask = match tiny_skia::Mask::new(self.pixmap.width(), self.pixmap.height()) {
            Some(mask) => mask,
            None => return,
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
                mask.fill_path(
                    &PathBuilder::from_rect(rect),
                    FillRule::Winding,
                    false,
                    Transform::identity(),
                );
            }
        }
        self.glyphs_over(paragraph, x, y, Some((mark.foreground, &mask)));
    }

    fn glyphs(
        &mut self,
        paragraph: &Paragraph,
        x: f32,
        y: f32,
        over: Option<(Option<Brush>, &tiny_skia::Mask)>,
    ) {
        self.glyphs_over(paragraph, x, y, over);
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
                        over.map(|(_, mask)| mask),
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

/// `pixel`, red, green, blue and alpha, with its colours multiplied by its
/// alpha, as tiny-skia holds pixels.
fn premultiplied([red, green, blue, alpha]: [u8; 4]) -> [u8; 4] {
    let scale = |channel: u8| ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8;
    [scale(red), scale(green), scale(blue), alpha]
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
            crate::reading::text::InlinePicture {
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
