use std::io;

use image::ImageFormat;

use crate::diagrams::{Renderer, Request};
use crate::images::tests::{Stub, fixtures, load_source, rgba, with_stub};
use crate::images::{DEFAULT_MAX_FILE_SIZE, ImageKey, decode, load_with};
use crate::typeset::images::drawn::*;

#[test]
fn a_diagram_that_fails_is_a_warning_naming_it() {
    let source = "# Plan\n\n```mermaid\nflowchart TD\n  broken\n```\n";
    let warnings = |by_line| {
        let stub = Stub::default();
        let images = load_source(source, &with_stub(&stub, Theme::Light, by_line));
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
    let images = load_source(
        "```dot\ndigraph {}\n```\n",
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
        &crate::typeset::parse::parse("```dot\ndigraph {}\n```\n"),
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
        let images = load_source(
            "```dot\ndigraph {}\n```\n",
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
    let images = load_source(
        "```dot\ndigraph {}\n```\n",
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
