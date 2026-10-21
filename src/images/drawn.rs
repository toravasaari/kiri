//! Diagrams made into images as their renderer drew them: decoded, grown
//! to show all of their text, reduced to the most that can be shown, and
//! laid on a panel when their colours are for the other page.

use image::RgbaImage;

use super::LoadOptions;
use super::decode::{decode_raster, scale};
use super::svg::{Fonts, decode_svg, looks_like_svg};
use super::{ImageError, Source};
use crate::diagrams::{Drawn, Theme};

/// The diagram `source` in `language`, which starts on `line`, as
/// `diagrams` drew it, decoded by its content as an image file is.
pub(super) fn finish_diagram(
    language: &str,
    source: &str,
    line: usize,
    drawn: Result<Drawn, String>,
    max_size: u64,
    fonts: &Fonts,
    options: &LoadOptions<'_>,
) -> Result<Source, ImageError> {
    let failed = |reason: String| ImageError::Diagram {
        name: crate::diagrams::name(language, source, line, options.by_line),
        reason,
    };
    let drawn = drawn.map_err(failed)?;
    let image = match looks_like_svg(&drawn.image) {
        true => decode_diagram(&drawn.image, max_size, fonts),
        false => decode_raster(&drawn.image, None)
            .map(|image| dense(image, drawn.scale, options.largest)),
    }
    .map_err(|error| failed(error.to_string()))?;
    // A diagram that chose colours of its own for the other page would
    // be dark lines and text on a dark page, or light ones on a light page.
    let panel = match (options.theme, drawn.theme) {
        _ if !options.panel => None,
        (Theme::Dark, Theme::Light) => Some(LIGHT_PANEL),
        (Theme::Light, Theme::Dark) => {
            let language = crate::diagrams::dark_background(language);
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
mod tests {
    use std::io;

    use image::ImageFormat;

    use super::*;
    use crate::diagrams::{Renderer, Request};
    use crate::images::tests::{Stub, fixtures, rgba, with_stub};
    use crate::images::{DEFAULT_MAX_FILE_SIZE, ImageKey, decode, load_with};

    #[test]
    fn a_diagram_that_fails_is_a_warning_naming_it() {
        let source = "# Plan\n\n```mermaid\nflowchart TD\n  broken\n```\n";
        let warnings = |by_line| {
            let stub = Stub::default();
            let images = load_with(
                &crate::parse::parse(source),
                &fixtures(),
                DEFAULT_MAX_FILE_SIZE,
                &Fonts::none(),
                &with_stub(&stub, Theme::Light, by_line),
            );
            images.warnings()
        };
        assert_eq!(warnings(true), ["mermaid diagram on line 3: parse error"]);
        assert_eq!(
            warnings(false),
            ["mermaid diagram \"flowchart TD\": parse error"]
        );
    }

    #[test]
    fn a_diagram_that_is_no_svg_is_a_warning_naming_it() {
        struct Text;
        impl Renderer for Text {
            fn draws(&self, _language: &str) -> bool {
                true
            }
            fn render(&self, request: &Request) -> Result<Drawn, String> {
                let theme = request.theme;
                Ok(Drawn::svg("not svg", theme, None))
            }
        }
        let images = load_with(
            &crate::parse::parse("```dot\ndigraph {}\n```\n"),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            &LoadOptions {
                renderer: &Text,
                ..LoadOptions::default()
            },
        );
        let warnings = images.warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("dot diagram on line 1: "),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_diagram_may_be_a_compressed_svg() {
        // As a program may write it, in bytes that are no UTF-8.
        struct Gzipped;
        impl Renderer for Gzipped {
            fn draws(&self, _language: &str) -> bool {
                true
            }
            fn render(&self, request: &Request) -> Result<Drawn, String> {
                let theme = request.theme;
                use std::io::Write;
                let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="70" height="30"/>"#;
                let mut gzip = flate2::write::GzEncoder::new(Vec::new(), Default::default());
                gzip.write_all(svg.as_bytes()).unwrap();
                Ok(Drawn {
                    image: gzip.finish().unwrap(),
                    scale: 1,
                    theme,
                    background: None,
                })
            }
        }
        let drawn = drawn_by(&Gzipped, Theme::Light);
        match drawn {
            Ok(Source::Svg(svg)) => assert_eq!(svg.size, (70, 30)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_diagram_grows_to_show_text_that_reaches_past_its_edges() {
        // A word that starts 20 pixels left of a 100 × 40 drawing.
        const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40">
            <text x="-20" y="30" font-size="20" font-family="sans-serif">Kiri</text></svg>"#;
        struct Overflowing;
        impl Renderer for Overflowing {
            fn draws(&self, _language: &str) -> bool {
                true
            }
            fn render(&self, request: &Request) -> Result<Drawn, String> {
                let theme = request.theme;
                Ok(Drawn::svg(SVG, theme, None))
            }
        }
        let fonts = Fonts::fixture();
        let images = load_with(
            &crate::parse::parse("```dot\ndigraph {}\n```\n"),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &fonts,
            &LoadOptions {
                renderer: &Overflowing,
                ..LoadOptions::default()
            },
        );
        let Some(Ok(Source::Svg(svg))) = images.get(&ImageKey::block("dot", "digraph {}\n")) else {
            panic!("{images:?}");
        };
        let (width, height) = svg.size;
        assert!((118..=120).contains(&width), "{width} pixels wide");
        assert_eq!(height, 40);
        // The K's stem is at the left edge.
        let drawn = svg.render(width, height);
        let mut left_edge = (0..3).flat_map(|x| (0..height).map(move |y| (x, y)));
        assert!(left_edge.any(|(x, y)| rgba(&drawn, x, y)[3] > 0));
        // An SVG file shows only what lies inside its edges, as in a browser.
        match decode(SVG.as_bytes(), DEFAULT_MAX_FILE_SIZE, &fonts) {
            Ok(Source::Svg(file)) => assert_eq!(file.size, (100, 40)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_diagram_for_the_other_page_lies_on_a_panel_of_its_background() {
        /// An empty 100 × 40 drawing, for a page of the theme it holds and
        /// with the background it holds.
        struct Themed(Theme, Option<[u8; 3]>);
        impl Renderer for Themed {
            fn draws(&self, _language: &str) -> bool {
                true
            }
            fn render(&self, _: &Request) -> Result<Drawn, String> {
                let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40"/>"#;
                Ok(Drawn::svg(svg, self.0, self.1))
            }
        }
        let drawn = |page, own, background, panel| {
            let images = load_with(
                &crate::parse::parse("```dot\ndigraph {}\n```\n"),
                &fixtures(),
                DEFAULT_MAX_FILE_SIZE,
                &Fonts::none(),
                &LoadOptions {
                    renderer: &Themed(own, background),
                    theme: page,
                    panel,
                    ..LoadOptions::default()
                },
            );
            match images.get(&ImageKey::block("dot", "digraph {}\n")) {
                Some(Ok(Source::Svg(svg))) => svg.clone(),
                other => panic!("{other:?}"),
            }
        };
        let navy = Some([0x12, 0x34, 0x56]);
        // Eight pixels of panel around the drawing on each side: white
        // under a light diagram, whatever its background, and its own
        // background, or a dark grey, under a dark diagram.
        for (page, own, background, colour) in [
            (Theme::Dark, Theme::Light, None, [255, 255, 255]),
            (Theme::Dark, Theme::Light, navy, [255, 255, 255]),
            (Theme::Light, Theme::Dark, navy, [0x12, 0x34, 0x56]),
            (Theme::Light, Theme::Dark, None, [0x33, 0x33, 0x33]),
        ] {
            let case = format!("{own:?} on {page:?} with {background:?}");
            let panel = drawn(page, own, background, true);
            assert_eq!(panel.size, (116, 56), "{case}");
            let pixels = panel.render(116, 56);
            for (x, y) in [(0, 0), (58, 28), (115, 55)] {
                let [red, green, blue] = colour;
                assert_eq!(rgba(&pixels, x, y), [red, green, blue, 255], "{case}");
            }
        }
        // kiri's own choice of theme, and no panels when they are off.
        for (page, own, panel) in [
            (Theme::Dark, Theme::Dark, true),
            (Theme::Light, Theme::Light, true),
            (Theme::Dark, Theme::Light, false),
            (Theme::Light, Theme::Dark, false),
        ] {
            let case = format!("{own:?} on {page:?}, panel {panel}");
            let svg = drawn(page, own, navy, panel);
            assert_eq!(svg.size, (100, 40), "{case}");
            assert_eq!(rgba(&svg.render(100, 40), 0, 0)[3], 0, "{case}");
        }
    }

    /// `image` as a PNG file.
    fn png(image: &RgbaImage) -> Vec<u8> {
        let mut bytes = io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    /// Draws every diagram as the PNG of a 60 × 30 pixel image, half
    /// transparent red, at its scale, for a page of its theme.
    struct Png(u32, Theme);

    impl Renderer for Png {
        fn draws(&self, _language: &str) -> bool {
            true
        }

        fn render(&self, _: &Request) -> Result<Drawn, String> {
            let image = RgbaImage::from_pixel(60, 30, image::Rgba([255, 0, 0, 128]));
            Ok(Drawn {
                image: png(&image),
                scale: self.0,
                theme: self.1,
                background: None,
            })
        }
    }

    /// The diagram that `renderer` draws, on a page of `page`.
    fn drawn_by(renderer: &dyn Renderer, page: Theme) -> Result<Source, ImageError> {
        drawn_within(renderer, page, None)
    }

    /// The diagram that `renderer` draws, on a page of `page`, where it is
    /// shown at most `largest` pixels large.
    fn drawn_within(
        renderer: &dyn Renderer,
        page: Theme,
        largest: Option<(u32, u32)>,
    ) -> Result<Source, ImageError> {
        let images = load_with(
            &crate::parse::parse("```dot\ndigraph {}\n```\n"),
            &fixtures(),
            DEFAULT_MAX_FILE_SIZE,
            &Fonts::none(),
            &LoadOptions {
                renderer,
                theme: page,
                largest,
                ..LoadOptions::default()
            },
        );
        (images.get(&ImageKey::block("dot", "digraph {}\n")).cloned()).expect("the diagram")
    }

    #[test]
    fn a_raster_diagram_larger_than_it_can_be_shown_is_reduced_as_it_loads() {
        // 60 × 30 pixels at three to the CSS pixel, 20 × 10 CSS pixels.
        let reduced = |largest, page| {
            let drawn = drawn_within(&Png(3, Theme::Light), page, Some(largest)).unwrap();
            let Source::Dense { image, .. } = &drawn else {
                panic!("{drawn:?}");
            };
            (image.dimensions(), drawn.size(), rgba(image, 0, 0))
        };
        let red = [255, 0, 0, 128];
        // At most 15 pixels wide: a quarter of the pixels each way, and
        // the same CSS pixels.
        assert_eq!(reduced((15, 100), Theme::Light), ((15, 8), (20, 10), red));
        // At most 15 pixels high: half of them.
        assert_eq!(reduced((100, 15), Theme::Light), ((30, 15), (20, 10), red));
        // One that fits stays as drawn.
        assert_eq!(reduced((60, 30), Theme::Light), ((60, 30), (20, 10), red));
        // Reduced before the panel goes under it: its eight CSS pixels are
        // six pixels at three quarters of a pixel to the CSS pixel.
        let white = [255, 255, 255, 255];
        assert_eq!(reduced((15, 100), Theme::Dark), ((27, 20), (36, 26), white));
    }

    #[test]
    fn a_raster_diagram_is_shown_at_the_size_of_its_css_pixels() {
        for scale in [1, 3] {
            let drawn = drawn_by(&Png(scale, Theme::Light), Theme::Light);
            let Ok(Source::Dense { image, css }) = &drawn else {
                panic!("{drawn:?}");
            };
            assert_eq!(
                (image.dimensions(), *css),
                ((60, 30), (60 / scale, 30 / scale))
            );
            let source = drawn.unwrap();
            assert_eq!(source.size(), (60 / scale, 30 / scale));
            assert_eq!(rgba(&source.render(60, 30), 5, 5), [255, 0, 0, 128]);
        }
    }

    #[test]
    fn a_raster_diagram_for_the_other_page_lies_on_a_panel() {
        // Eight CSS pixels of panel on each side, and the drawing over it.
        for scale in [1, 3] {
            let drawn = drawn_by(&Png(scale, Theme::Light), Theme::Dark).unwrap();
            let pad = 8 * scale;
            assert_eq!(drawn.size(), (60 / scale + 16, 30 / scale + 16));
            let Source::Dense { image, .. } = &drawn else {
                panic!("{drawn:?}");
            };
            assert_eq!(image.dimensions(), (60 + 2 * pad, 30 + 2 * pad));
            assert_eq!(rgba(image, 0, 0), [255, 255, 255, 255]);
            assert_eq!(rgba(image, pad + 5, pad + 5), [255, 127, 127, 255]);
            let drawn = drawn_by(&Png(scale, Theme::Dark), Theme::Light).unwrap();
            let Source::Dense { image, .. } = &drawn else {
                panic!("{drawn:?}");
            };
            assert_eq!(rgba(image, 0, 0), [0x33, 0x33, 0x33, 255]);
        }
    }
}
