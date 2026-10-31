//! Formulas in LaTeX, as ` ```math ` blocks and `$$…$$` give them, drawn
//! by RaTeX inside kiri.

use ratex_layout::{LayoutOptions, layout, to_display_list, validate_layout};
use ratex_svg::{SvgOptions, render_to_svg};
use ratex_types::{Color, MathStyle};

use crate::typeset::diagrams::error::DiagramError;
use crate::typeset::diagrams::language::{self, BuiltInRenderer, Language, ThemeNames};
use crate::typeset::diagrams::request::{Drawn, Request, Theme};

/// Formulas.
pub(super) const MATH: Language = Language {
    name: "math",
    aliases: &[],
    display_name: "math",
    noun: Some("formula"),
    centred: true,
    file_extensions: &[],
    source_extension: "tex",
    theme_names: ThemeNames::PLAIN,
    default_command: None,
    config: None,
    chosen_page: None,
    first_line: language::first_line,
    dark_background: None,
    built_in: Some(BuiltInRenderer {
        version,
        render,
        baseline,
    }),
};

/// A formula in the middle of text.
pub(super) const MATH_INLINE: Language = Language {
    name: crate::typeset::diagrams::request::INLINE_FORMULA,
    centred: false,
    ..MATH
};

/// RaTeX's version. Cargo tells a crate only its own version, so a test
/// keeps this one in step with `Cargo.lock`.
const RATEX_VERSION: &str = "0.1.15";

/// How large a formula block's letters are, in CSS pixels to the em:
/// KaTeX's 1.21 em of a browser's body text, 16 CSS pixels, as GitHub
/// draws a formula block.
const BLOCK_FONT_SIZE: f64 = 16.0 * 1.21;

/// How large an inline formula's letters are: those of the text around
/// it, which is 16 CSS pixels to the em.
const INLINE_FONT_SIZE: f64 = 16.0;

/// The room around a formula, in CSS pixels, so that the edges of its
/// letters are not cut off.
const PADDING: f64 = 2.0;

/// RaTeX gives an SVG's size in points, which are 4/3 of a CSS pixel.
const POINTS_PER_CSS_PIXEL: f64 = 0.75;

/// The fraction of a formula's image, from its bottom, that is below its
/// baseline.
fn baseline(source: &str) -> Option<f64> {
    let nodes = ratex_parser::parse(source).ok()?;
    let options = LayoutOptions::default().with_style(MathStyle::Text);
    let tree = layout(&nodes, &options);
    validate_layout(&tree).ok()?;
    let list = to_display_list(&tree);
    let (em, pad) = (INLINE_FONT_SIZE, PADDING);
    Some((list.depth * em + pad) / ((list.height + list.depth) * em + 2.0 * pad))
}

/// RaTeX's version.
fn version() -> String {
    format!("RaTeX {RATEX_VERSION}")
}

/// The formula that `request` asks for as an SVG document, its glyphs as
/// paths, in the colours of the request, on a transparent background
/// unless they have one; or RaTeX's message saying why it could not be
/// drawn.
fn render(request: &Request) -> Result<Drawn, DiagramError> {
    let nodes = ratex_parser::parse(request.source)
        .map_err(|error| DiagramError::from(error.to_string()))?;
    let [red, green, blue] = request.formula.text.unwrap_or(match request.theme {
        Theme::Dark => [0xff, 0xff, 0xff],
        Theme::Light => [0x00, 0x00, 0x00],
    });
    let channel = |value: u8| f32::from(value) / 255.0;
    // A formula in the middle of text is in text style at the size of the
    // text; one of its own is larger, in display style.
    let (style, size) = match request.language == crate::typeset::diagrams::request::INLINE_FORMULA
    {
        true => (MathStyle::Text, INLINE_FONT_SIZE),
        false => (MathStyle::Display, BLOCK_FONT_SIZE),
    };
    let options = (LayoutOptions::default())
        .with_style(style)
        .with_color(Color::rgb(channel(red), channel(green), channel(blue)));
    let tree = layout(&nodes, &options);
    validate_layout(&tree).map_err(DiagramError::from)?;
    let svg = render_to_svg(
        &to_display_list(&tree),
        &SvgOptions {
            font_size: size * POINTS_PER_CSS_PIXEL,
            padding: PADDING * POINTS_PER_CSS_PIXEL,
            embed_glyphs: true,
            ..SvgOptions::default()
        },
    );
    let svg = match request.formula.background {
        Some(background) => on_background(&svg, background),
        None => svg,
    };
    Ok(Drawn::svg(svg, request.theme, None))
}

/// `svg` drawn over a rectangle of `colour` as large as itself.
fn on_background(svg: &str, [red, green, blue]: [u8; 3]) -> String {
    let rect =
        format!("<rect width=\"100%\" height=\"100%\" fill=\"#{red:02x}{green:02x}{blue:02x}\"/>");
    match svg.find('>') {
        Some(end) => format!("{}{rect}{}", &svg[..=end], &svg[end + 1..]),
        None => svg.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::Fonts;
    use crate::typeset::diagrams::request::FormulaColors;
    use crate::typeset::images::{DEFAULT_MAX_FILE_SIZE, decode};

    fn drawn(source: &str, theme: Theme, color: Option<[u8; 3]>) -> Result<String, DiagramError> {
        let colors = FormulaColors {
            text: color,
            background: None,
        };
        let request = Request::new("math", source, theme).in_colors(colors);
        render(&request).map(|drawn| drawn.text())
    }

    #[test]
    fn the_ratex_version_is_the_one_in_cargo_lock() {
        let lock = include_str!("../../../Cargo.lock");
        for name in ["ratex-parser", "ratex-layout", "ratex-svg", "ratex-types"] {
            let entry = format!("name = \"{name}\"\nversion = \"{RATEX_VERSION}\"\n");
            assert!(lock.contains(&entry), "{entry}");
        }
        assert_eq!(version(), format!("RaTeX {RATEX_VERSION}"));
    }

    #[test]
    fn draws_a_formula_as_svg_without_text() {
        let svg = drawn("\\frac{a}{b} + \\sum_{i=1}^n i", Theme::Light, None).unwrap();
        assert!(svg.starts_with("<svg"), "{svg}");
        assert!(svg.contains("<path"), "{svg}");
        // Glyphs as paths: no fonts are needed to show it.
        assert!(!svg.contains("<text"), "{svg}");
    }

    #[test]
    fn draws_in_the_colour_asked_for_on_no_background() {
        let svg = drawn("x^2", Theme::Light, Some([0x12, 0x34, 0x56])).unwrap();
        assert!(svg.contains("rgba(18,52,86,1)"), "{svg}");
        assert!(!svg.contains("<rect"), "{svg}");
    }

    #[test]
    fn without_a_colour_draws_white_on_a_dark_page_and_black_on_a_light_one() {
        let dark = drawn("x", Theme::Dark, None).unwrap();
        assert!(dark.contains("rgba(255,255,255,1)"), "{dark}");
        let light = drawn("x", Theme::Light, None).unwrap();
        assert!(light.contains("rgba(0,0,0,1)"), "{light}");
    }

    #[test]
    fn draws_on_the_background_asked_for() {
        let colors = FormulaColors {
            text: Some([0, 0, 0]),
            background: Some([0x10, 0x20, 0x30]),
        };
        let request = Request::new("math", "x", Theme::Light).in_colors(colors);
        let svg = render(&request).unwrap().text();
        let image = decode(svg.as_bytes(), DEFAULT_MAX_FILE_SIZE, &Fonts::none()).unwrap();
        let (width, height) = image.size();
        let drawn = image.render(width, height);
        for (x, y) in [(0, 0), (width - 1, height - 1)] {
            assert_eq!(drawn.get_pixel(x, y).0, [0x10, 0x20, 0x30, 255]);
        }
    }

    #[test]
    fn a_formula_block_is_as_large_as_katex_draws_one() {
        // KaTeX's 1.21 em of 16 CSS pixels, the browser's body text: a
        // rule 100 em wide is 1936 pixels, with 2 of padding on each side.
        let svg = drawn("\\rule{100em}{1em}", Theme::Light, None).unwrap();
        let image = decode(svg.as_bytes(), DEFAULT_MAX_FILE_SIZE, &Fonts::none()).unwrap();
        let (width, _) = image.size();
        assert_eq!(width, 1936 + 2 * 2);
    }

    /// The size of the image of `source` as `language` draws it.
    fn size_of(language: &str, source: &str) -> (u32, u32) {
        let request = Request::new(language, source, Theme::Light);
        let svg = render(&request).unwrap().text();
        decode(svg.as_bytes(), DEFAULT_MAX_FILE_SIZE, &Fonts::none())
            .unwrap()
            .size()
    }

    #[test]
    fn an_inline_formula_is_as_large_as_the_text_around_it() {
        // 16 CSS pixels to the em, as the text is: a rule 100 em wide is
        // 1600 pixels, with 2 of padding on each side.
        assert_eq!(size_of("math-inline", "\\rule{100em}{1em}").0, 1600 + 4);
    }

    #[test]
    fn an_inline_formula_is_in_text_style() {
        // A fraction in text style is smaller than in display style, which
        // is the style of the block.
        let source = "\\frac{a}{b}";
        let inline = size_of("math-inline", source).1;
        let block = size_of("math", source).1;
        assert!(inline * 10 < block * 8, "{inline} of {block}");
    }

    #[test]
    fn a_formula_tells_how_far_below_its_baseline_it_reaches() {
        // A rule 1 em high on the baseline, 16 pixels with 2 of padding
        // on each side, has only the padding below it.
        let depth = baseline("\\rule{1em}{1em}").unwrap();
        assert!((depth - 2.0 / 20.0).abs() < 1e-6, "{depth}");
        // A fraction reaches further below, as the line of its bar is
        // above the baseline and its denominator below it.
        let fraction = baseline("\\frac{a}{b}").unwrap();
        assert!(fraction > 0.3 && fraction < 0.6, "{fraction}");
        assert!(baseline("\\frac{").is_none());
    }

    #[test]
    fn a_formula_with_a_mistake_is_ratex_s_message() {
        let error = drawn("\\frac{", Theme::Light, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("ParseError"), "{error}");
        let error = drawn("\\nosuchcommand", Theme::Light, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("nosuchcommand"), "{error}");
    }
}
