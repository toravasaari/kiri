//! Diagrams made into images as their renderer drew them: decoded, grown
//! to show all of their text, reduced to the most that can be shown, and
//! laid on a panel when their colours are for the other page.

use image::RgbaImage;

use crate::images::svg::{Fonts, decode_svg, looks_like_svg};
use crate::typeset::diagrams::error::DiagramError;
use crate::typeset::diagrams::request::{Drawn, Theme};
use crate::typeset::images::LoadOptions;
use crate::typeset::images::decode::{decode_raster, scale};
use crate::typeset::images::{ImageError, Source};

/// The diagram `source` in `language`, which starts on `line`, as
/// `diagrams` drew it, decoded by its content as an image file is.
pub(crate) fn finish_diagram(
    language: &str,
    source: &str,
    line: usize,
    drawn: Result<Drawn, DiagramError>,
    max_size: u64,
    fonts: &Fonts,
    options: &LoadOptions<'_>,
) -> Result<Source, ImageError> {
    let failed = |reason: DiagramError| ImageError::Diagram {
        name: crate::typeset::diagrams::names::name(language, source, line, options.by_line),
        reason,
    };
    let drawn = drawn.map_err(failed)?;
    let image = match looks_like_svg(&drawn.image) {
        true => decode_diagram(&drawn.image, max_size, fonts),
        false => decode_raster(&drawn.image, None)
            .map(|image| dense(image, drawn.scale, options.largest)),
    }
    .map_err(|error| failed(error.to_string().into()))?;
    // A diagram that chose colours of its own for the other page would
    // be dark lines and text on a dark page, or light ones on a light page.
    let panel = match (options.theme, drawn.theme) {
        _ if !options.panel => None,
        (Theme::Dark, Theme::Light) => Some(LIGHT_PANEL),
        (Theme::Light, Theme::Dark) => {
            let language = crate::typeset::diagrams::names::dark_background(language);
            Some(drawn.background.or(language).unwrap_or(DARK_PANEL))
        }
        (Theme::Light, Theme::Light) | (Theme::Dark, Theme::Dark) => None,
    };
    Ok(match (image, panel) {
        (Source::Svg(svg), Some(colour)) => Source::Svg(svg.on_panel(colour, PANEL_PADDING)),
        (Source::Dense { image, css }, Some(colour)) => {
            let per_css = f64::from(image.width()) / f64::from(css.0.max(1));
            let padding = (f64::from(PANEL_PADDING) * per_css).round() as u32;
            let grown = |length: u32| length.saturating_add(2 * PANEL_PADDING as u32);
            Source::Dense {
                image: on_panel(&image, colour, padding),
                css: (grown(css.0), grown(css.1)),
            }
        }
        (image, _) => image,
    })
}

/// `image`, drawn at `per_css` pixels to the CSS pixel, as a dense image
/// of as many CSS pixels; reduced, keeping its shape, when it has more
/// pixels than fit in `largest`, as large as it can be shown, so that a
/// large diagram holds no more memory than showing it needs.
fn dense(image: RgbaImage, per_css: u32, largest: Option<(u32, u32)>) -> Source {
    let (width, height) = image.dimensions();
    let per_css = per_css.max(1);
    let css = (width.div_ceil(per_css), height.div_ceil(per_css));
    let factor = largest.map_or(1.0, |(most_wide, most_high)| {
        let wide = f64::from(most_wide.max(1)) / f64::from(width.max(1));
        let high = f64::from(most_high.max(1)) / f64::from(height.max(1));
        wide.min(high)
    });
    if factor >= 1.0 {
        return Source::Dense { image, css };
    }
    let reduced = |length: u32| ((f64::from(length) * factor).round() as u32).max(1);
    Source::Dense {
        image: scale(&image, reduced(width), reduced(height)),
        css,
    }
}

/// `image` over a panel of `colour`, `padding` pixels wider than it on
/// each side.
fn on_panel(image: &RgbaImage, colour: [u8; 3], padding: u32) -> RgbaImage {
    let [red, green, blue] = colour;
    let (width, height) = image.dimensions();
    let grown = |length: u32| length.saturating_add(padding.saturating_mul(2));
    let mut panel = RgbaImage::from_pixel(
        grown(width),
        grown(height),
        image::Rgba([red, green, blue, 255]),
    );
    for (x, y, pixel) in image.enumerate_pixels() {
        let [r, g, b, alpha] = pixel.0;
        let over = |top: u8, under: u8| {
            let mixed =
                u32::from(top) * u32::from(alpha) + u32::from(under) * (255 - u32::from(alpha));
            ((mixed + 127) / 255) as u8
        };
        let below = panel.get_pixel_mut(x + padding, y + padding);
        below.0 = [over(r, red), over(g, green), over(b, blue), 255];
    }
    panel
}

/// The colour of the panel under a light diagram on a dark page.
const LIGHT_PANEL: [u8; 3] = [255, 255, 255];

/// The colour of the panel under a dark diagram on a light page, when
/// neither its renderer nor its language tells its background: a dark
/// grey.
const DARK_PANEL: [u8; 3] = [0x33, 0x33, 0x33];

/// How far, in CSS pixels, the panel reaches past the diagram on each side,
/// beyond the margin of its own that the diagram has.
const PANEL_PADDING: f32 = 8.0;

/// A diagram's SVG decoded, and grown to show all of its text: its
/// renderer may have measured the text narrower than `fonts` draw it.
pub(crate) fn decode_diagram(
    svg: &[u8],
    max_size: u64,
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    decode_svg(svg, max_size, fonts).map(|svg| Source::Svg(svg.grown_to_text()))
}

#[cfg(test)]
mod tests;
