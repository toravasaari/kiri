//! Formulas in LaTeX, as ` ```math ` blocks and `$$…$$` give them, drawn
//! by RaTeX inside kiri.

use ratex_layout::{LayoutOptions, layout, to_display_list, validate_layout};
use ratex_svg::{SvgOptions, render_to_svg};
use ratex_types::{Color, MathStyle};

use super::language::{self, BuiltInRenderer, Language, ThemeNames};
use super::{Drawn, Request, Theme};

/// Formulas.
pub(super) const MATH: Language = Language {
    name: "math",
    aliases: &[],
    display_name: "math",
    noun: Some("formula"),
    file_extensions: &[],
    source_extension: "tex",
    theme_names: ThemeNames::PLAIN,
    default_command: None,
    config: None,
    chosen_page: None,
    first_line: language::first_line,
    dark_background: None,
    built_in: Some(BuiltInRenderer { version, render }),
};

/// RaTeX's version. Cargo tells a crate only its own version, so a test
/// keeps this one in step with `Cargo.lock`.
const RATEX_VERSION: &str = "0.1.15";

/// How large a formula's letters are, in CSS pixels to the em: the size of
/// body text in a browser.
const FONT_SIZE: f64 = 16.0;

/// The room around a formula, in CSS pixels, so that the edges of its
/// letters are not cut off.
const PADDING: f64 = 2.0;

/// RaTeX gives an SVG's size in points, which are 4/3 of a CSS pixel.
const POINTS_PER_CSS_PIXEL: f64 = 0.75;

/// RaTeX's version.
fn version() -> String {
    format!("RaTeX {RATEX_VERSION}")
}

/// The formula that `request` asks for as an SVG document, its glyphs as
/// paths, in the colour of the request on a transparent background; or
/// RaTeX's message saying why it could not be drawn.
fn render(request: &Request) -> Result<Drawn, String> {
    let nodes = ratex_parser::parse(request.source).map_err(|error| error.to_string())?;
    let [red, green, blue] = request.color.unwrap_or(match request.theme {
        Theme::Dark => [0xff, 0xff, 0xff],
        Theme::Light => [0x00, 0x00, 0x00],
    });
    let channel = |value: u8| f32::from(value) / 255.0;
    let options = (LayoutOptions::default())
        .with_style(MathStyle::Display)
        .with_color(Color::rgb(channel(red), channel(green), channel(blue)));
    let tree = layout(&nodes, &options);
    validate_layout(&tree)?;
    let svg = render_to_svg(
        &to_display_list(&tree),
        &SvgOptions {
            font_size: FONT_SIZE * POINTS_PER_CSS_PIXEL,
            padding: PADDING * POINTS_PER_CSS_PIXEL,
            embed_glyphs: true,
            ..SvgOptions::default()
        },
    );
    Ok(Drawn::svg(svg, request.theme, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::{DEFAULT_MAX_FILE_SIZE, Fonts, decode};

    fn drawn(source: &str, theme: Theme, color: Option<[u8; 3]>) -> Result<String, String> {
        let request = Request::new("math", source, theme).in_color(color);
        render(&request).map(|drawn| drawn.text())
    }

    #[test]
    fn the_ratex_version_is_the_one_in_cargo_lock() {
        let lock = include_str!("../../Cargo.lock");
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
    fn an_em_is_sixteen_css_pixels() {
        // A rule as wide as 10 em, with the padding on each side.
        let svg = drawn("\\rule{10em}{1em}", Theme::Light, None).unwrap();
        let image = decode(svg.as_bytes(), DEFAULT_MAX_FILE_SIZE, &Fonts::none()).unwrap();
        let (width, _) = image.size();
        assert_eq!(f64::from(width), 10.0 * FONT_SIZE + 2.0 * PADDING);
    }

    #[test]
    fn a_formula_with_a_mistake_is_ratex_s_message() {
        let error = drawn("\\frac{", Theme::Light, None).unwrap_err();
        assert!(error.contains("ParseError"), "{error}");
        let error = drawn("\\nosuchcommand", Theme::Light, None).unwrap_err();
        assert!(error.contains("nosuchcommand"), "{error}");
    }
}
