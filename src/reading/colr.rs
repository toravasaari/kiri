//! Colour glyphs, as emoji fonts such as Noto Color Emoji draw them with
//! the COLR table: layers of shapes filled with colours and gradients from
//! the font's palette, painted with tiny-skia.

use resvg::tiny_skia::{
    BlendMode, Color, FillRule, GradientStop, LinearGradient, Mask, Paint, PathBuilder, Pixmap,
    PixmapPaint, Point, RadialGradient, Rect, Shader, SpreadMode, Transform,
};
use skrifa::color::{Brush, ColorPainter, ColorStop, CompositeMode, Extend};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::types::BoundingBox;
use skrifa::{FontRef, GlyphId, MetadataProvider};

/// Draws the colour glyph `glyph` of `font` at `size` pixels with its
/// origin at (`x`, `y`) on `target`, in the font's first palette; the
/// palette's text colour is `text`. False when the font has no colour
/// glyph for it, and nothing is drawn.
pub fn draw(
    target: &mut Pixmap,
    font: &FontRef,
    glyph: u32,
    size: f32,
    (x, y): (f32, f32),
    text: [u8; 3],
) -> bool {
    let Some(color_glyph) = font.color_glyphs().get(GlyphId::new(glyph)) else {
        return false;
    };
    let upem = font
        .metrics(Size::unscaled(), LocationRef::default())
        .units_per_em;
    let scale = size / f32::from(upem.max(1));
    // A glyph is drawn on a layer of its own around its origin, an em
    // above and half an em below the baseline and two ems wide, which is
    // then laid on the target: a canvas-wide layer per glyph would be slow.
    let left = (x - size * 0.5).floor();
    let top = (y - size * 1.25).floor();
    let (width, height) = ((size * 3.0).ceil() as u32, (size * 2.0).ceil() as u32);
    let Some(layer) = Pixmap::new(width.max(1), height.max(1)) else {
        return false;
    };
    let palettes = font.color_palettes();
    let palette = palettes.get(0);
    let colors: Vec<[u8; 4]> = (palette.iter())
        .flat_map(|palette| palette.colors())
        .map(|color| [color.red, color.green, color.blue, color.alpha])
        .collect();
    let mut painter = Painter {
        font,
        colors,
        text,
        base: Transform::from_row(scale, 0.0, 0.0, -scale, x - left, y - top),
        transforms: Vec::new(),
        clips: Vec::new(),
        layers: vec![(layer, CompositeMode::SrcOver)],
    };
    if color_glyph
        .paint(LocationRef::default(), &mut painter)
        .is_err()
    {
        return false;
    }
    let (layer, _) = painter.layers.swap_remove(0);
    target.draw_pixmap(
        left as i32,
        top as i32,
        layer.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    true
}

struct Painter<'a> {
    font: &'a FontRef<'a>,
    /// The colours of the font's first palette, as RGBA.
    colors: Vec<[u8; 4]>,
    /// The colour of the text, which a palette index of 0xFFFF names.
    text: [u8; 3],
    /// From the font's units, y up, to the layer's pixels, y down.
    base: Transform,
    transforms: Vec<Transform>,
    clips: Vec<Mask>,
    /// The layers being drawn, each with the mode that lays it on the one
    /// below; the first is the glyph's own.
    layers: Vec<(Pixmap, CompositeMode)>,
}

impl Painter<'_> {
    fn transform(&self) -> Transform {
        self.transforms.last().copied().unwrap_or(self.base)
    }

    fn layer(&mut self) -> &mut Pixmap {
        &mut self.layers.last_mut().expect("the glyph's own layer").0
    }

    fn size(&self) -> (u32, u32) {
        let (layer, _) = &self.layers[0];
        (layer.width(), layer.height())
    }

    /// The clip so far, narrowed to `path` drawn with the current
    /// transform.
    fn clip(&mut self, path: Option<resvg::tiny_skia::Path>) {
        let transform = self.transform();
        let (width, height) = self.size();
        let mask = match (self.clips.last(), path) {
            (Some(clip), Some(path)) => {
                let mut mask = clip.clone();
                mask.intersect_path(&path, FillRule::Winding, true, transform);
                Some(mask)
            }
            (None, Some(path)) => Mask::new(width, height).map(|mut mask| {
                mask.fill_path(&path, FillRule::Winding, true, transform);
                mask
            }),
            // A shape without a path covers nothing.
            (_, None) => Mask::new(width, height),
        };
        if let Some(mask) = mask {
            self.clips.push(mask);
        }
    }

    fn color(&self, index: u16, alpha: f32) -> Color {
        let [red, green, blue, a] = match index {
            0xFFFF => [self.text[0], self.text[1], self.text[2], 255],
            index => (self.colors.get(usize::from(index)).copied()).unwrap_or([0, 0, 0, 255]),
        };
        let alpha = (f32::from(a) / 255.0 * alpha).clamp(0.0, 1.0);
        Color::from_rgba(
            f32::from(red) / 255.0,
            f32::from(green) / 255.0,
            f32::from(blue) / 255.0,
            alpha,
        )
        .unwrap_or(Color::BLACK)
    }

    fn stops(&self, stops: &[ColorStop]) -> Vec<GradientStop> {
        (stops.iter())
            .map(|stop| GradientStop::new(stop.offset, self.color(stop.palette_index, stop.alpha)))
            .collect()
    }

    fn shader(&self, brush: &Brush) -> Option<Shader<'static>> {
        let transform = self.transform();
        let point = |p: skrifa::raw::types::Point<f32>| Point::from_xy(p.x, p.y);
        match brush {
            Brush::Solid {
                palette_index,
                alpha,
            } => Some(Shader::SolidColor(self.color(*palette_index, *alpha))),
            Brush::LinearGradient {
                p0,
                p1,
                color_stops,
                extend,
            } => LinearGradient::new(
                point(*p0),
                point(*p1),
                self.stops(color_stops),
                spread(*extend),
                transform,
            ),
            Brush::RadialGradient {
                c0,
                r0,
                c1,
                r1,
                color_stops,
                extend,
            } => RadialGradient::new(
                point(*c0),
                *r0,
                point(*c1),
                *r1,
                self.stops(color_stops),
                spread(*extend),
                transform,
            ),
            // tiny-skia has no sweep gradients: the colour halfway round
            // stands in.
            Brush::SweepGradient { color_stops, .. } => {
                let middle = color_stops.get(color_stops.len() / 2)?;
                Some(Shader::SolidColor(
                    self.color(middle.palette_index, middle.alpha),
                ))
            }
        }
        // A gradient of one stop, or of no length, is its colour.
        .or_else(|| {
            let stops = match brush {
                Brush::LinearGradient { color_stops, .. }
                | Brush::RadialGradient { color_stops, .. }
                | Brush::SweepGradient { color_stops, .. } => color_stops,
                Brush::Solid { .. } => return None,
            };
            let stop = stops.last()?;
            Some(Shader::SolidColor(
                self.color(stop.palette_index, stop.alpha),
            ))
        })
    }
}

impl ColorPainter for Painter<'_> {
    fn push_transform(&mut self, transform: skrifa::color::Transform) {
        let inner = Transform::from_row(
            transform.xx,
            transform.yx,
            transform.xy,
            transform.yy,
            transform.dx,
            transform.dy,
        );
        let combined = self.transform().pre_concat(inner);
        self.transforms.push(combined);
    }

    fn pop_transform(&mut self) {
        self.transforms.pop();
    }

    fn push_clip_glyph(&mut self, glyph: GlyphId) {
        let mut path = PathBuilder::new();
        if let Some(outline) = self.font.outline_glyphs().get(glyph) {
            let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
            let _ = outline.draw(settings, &mut Pen(&mut path));
        }
        self.clip(path.finish());
    }

    fn push_clip_box(&mut self, clip: BoundingBox<f32>) {
        let rect = Rect::from_ltrb(clip.x_min, clip.y_min, clip.x_max, clip.y_max);
        self.clip(rect.map(PathBuilder::from_rect));
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn fill(&mut self, brush: Brush<'_>) {
        let Some(shader) = self.shader(&brush) else {
            return;
        };
        let (width, height) = self.size();
        let paint = Paint {
            shader,
            anti_alias: true,
            ..Paint::default()
        };
        let rect = Rect::from_xywh(0.0, 0.0, width as f32, height as f32).expect("a size");
        let clip = self.clips.last().cloned();
        (self.layer()).fill_rect(rect, &paint, Transform::identity(), clip.as_ref());
    }

    fn push_layer(&mut self, mode: CompositeMode) {
        let (width, height) = self.size();
        if let Some(layer) = Pixmap::new(width, height) {
            self.layers.push((layer, mode));
        }
    }

    fn pop_layer(&mut self) {
        if self.layers.len() < 2 {
            return;
        }
        let (layer, mode) = self.layers.pop().expect("a layer above the glyph's");
        let paint = PixmapPaint {
            blend_mode: blend(mode),
            ..PixmapPaint::default()
        };
        (self.layer()).draw_pixmap(0, 0, layer.as_ref(), &paint, Transform::identity(), None);
    }
}

fn spread(extend: Extend) -> SpreadMode {
    match extend {
        Extend::Repeat => SpreadMode::Repeat,
        Extend::Reflect => SpreadMode::Reflect,
        _ => SpreadMode::Pad,
    }
}

fn blend(mode: CompositeMode) -> BlendMode {
    match mode {
        CompositeMode::Clear => BlendMode::Clear,
        CompositeMode::Src => BlendMode::Source,
        CompositeMode::Dest => BlendMode::Destination,
        CompositeMode::DestOver => BlendMode::DestinationOver,
        CompositeMode::SrcIn => BlendMode::SourceIn,
        CompositeMode::DestIn => BlendMode::DestinationIn,
        CompositeMode::SrcOut => BlendMode::SourceOut,
        CompositeMode::DestOut => BlendMode::DestinationOut,
        CompositeMode::SrcAtop => BlendMode::SourceAtop,
        CompositeMode::DestAtop => BlendMode::DestinationAtop,
        CompositeMode::Xor => BlendMode::Xor,
        CompositeMode::Plus => BlendMode::Plus,
        CompositeMode::Screen => BlendMode::Screen,
        CompositeMode::Overlay => BlendMode::Overlay,
        CompositeMode::Darken => BlendMode::Darken,
        CompositeMode::Lighten => BlendMode::Lighten,
        CompositeMode::ColorDodge => BlendMode::ColorDodge,
        CompositeMode::ColorBurn => BlendMode::ColorBurn,
        CompositeMode::HardLight => BlendMode::HardLight,
        CompositeMode::SoftLight => BlendMode::SoftLight,
        CompositeMode::Difference => BlendMode::Difference,
        CompositeMode::Exclusion => BlendMode::Exclusion,
        CompositeMode::Multiply => BlendMode::Multiply,
        CompositeMode::HslHue => BlendMode::Hue,
        CompositeMode::HslSaturation => BlendMode::Saturation,
        CompositeMode::HslColor => BlendMode::Color,
        CompositeMode::HslLuminosity => BlendMode::Luminosity,
        _ => BlendMode::SourceOver,
    }
}

/// Turns an outline in the font's units into a path in the same units.
struct Pen<'a>(&'a mut PathBuilder);

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.0.quad_to(cx, cy, x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.cubic_to(cx0, cy0, cx1, cy1, x, y);
    }

    fn close(&mut self) {
        self.0.close();
    }
}
