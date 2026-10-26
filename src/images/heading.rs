//! Headings drawn as images of their text, for a terminal that draws
//! images but cannot draw large text: each word starts on a whole cell, so
//! that a heading too long for the room can show the left part of its one
//! image, up to the last word that fits.

use super::svg::{Fonts, Svg, decode_svg};
use super::{ImageError, Source};

/// How the headings of each level are drawn, resolved from the theme and
/// the terminal by the renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadingLook {
    /// The size of one cell in pixels, width and height.
    pub cell: (u16, u16),
    /// The look of each level, `h1` first.
    pub levels: [LevelLook; 6],
}

impl HeadingLook {
    /// The look of headings of `level`, from 1 to 6.
    pub fn level(&self, level: u8) -> &LevelLook {
        &self.levels[usize::from(level.clamp(1, 6)) - 1]
    }
}

/// How the headings of one level are drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelLook {
    /// The rows the image covers, as many as the sized heading would.
    pub rows: u8,
    /// The size of the font in pixels.
    pub font_px: f32,
    pub bold: bool,
    pub italic: bool,
    /// The colour of the text, as RGB.
    pub color: [u8; 3],
}

/// The words of a heading's text, split on whitespace: the parts its image
/// draws, each from a whole cell.
pub fn heading_words(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

/// `text` drawn as `look` says with `fonts`, in cells of `cell` pixels:
/// one image `look.rows` cells high and a whole number of cells wide,
/// each word starting on a whole cell, with the columns the words after
/// the first start on. An error when no font draws the text.
pub fn draw_heading(
    text: &str,
    look: &LevelLook,
    cell: (u16, u16),
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    let words = heading_words(text);
    let cell_width = f32::from(cell.0);
    let height = u32::from(look.rows) * u32::from(cell.1);
    // The text sits on the bottom row as a sized heading's does, with
    // room below the baseline for the descenders.
    let baseline = height as f32 - DESCENT * look.font_px;
    let svg = |width: u32, texts: &[(f32, &str)]| {
        let svg = heading_svg(texts, look, (width, height), baseline);
        decode_svg(svg.as_bytes(), u64::MAX, fonts)
    };
    // How far the ink of each word reaches from where the word starts.
    let mut reaches = Vec::with_capacity(words.len());
    for word in &words {
        let edges = svg(1, &[(0.0, word)])?.text_edges();
        reaches.push(edges.map(|(_, right)| right.max(0.0)));
    }
    if reaches.iter().all(Option::is_none) {
        return Err(no_font());
    }
    let space = space_width(&svg).unwrap_or(look.font_px * SPACE);
    let mut starts: Vec<u16> = Vec::with_capacity(words.len());
    let mut end = 0.0_f32;
    for reach in &reaches {
        let start = match starts.last() {
            None => 0,
            Some(_) => ((end + space) / cell_width).ceil() as u16,
        };
        end = f32::from(start) * cell_width + reach.unwrap_or(0.0);
        starts.push(start);
    }
    let columns = ((end / cell_width).ceil() as u32).max(1);
    let width = columns * u32::from(cell.0);
    let texts: Vec<(f32, &str)> = (starts.iter())
        .zip(&words)
        .map(|(&start, &word)| (f32::from(start) * cell_width, word))
        .collect();
    let image = svg(width, &texts)?.render(width, height);
    Ok(Source::Text {
        image,
        breaks: starts.into_iter().skip(1).collect(),
    })
}

/// How far below the baseline the text reaches, in ems: the descent of
/// the usual sans fonts.
const DESCENT: f32 = 0.24;

/// The width of a space in ems, when no font tells.
const SPACE: f32 = 0.3;

/// The width of a space in the font `svg` draws with: how much farther
/// `x x` reaches than `xx`.
fn space_width(svg: &impl Fn(u32, &[(f32, &str)]) -> Result<Svg, ImageError>) -> Option<f32> {
    let reach = |text| Some(svg(1, &[(0.0, text)]).ok()?.text_edges()?.1);
    Some(reach("x x")? - reach("xx")?).filter(|&width| width > 0.0)
}

/// An SVG `size` pixels large with each of `texts` at its x and on
/// `baseline`, drawn as `look` says.
fn heading_svg(texts: &[(f32, &str)], look: &LevelLook, size: (u32, u32), baseline: f32) -> String {
    let [red, green, blue] = look.color;
    let weight = if look.bold { "bold" } else { "normal" };
    let style = if look.italic { "italic" } else { "normal" };
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">",
        size.0, size.1
    );
    for (x, text) in texts {
        svg.push_str(&format!(
            "<text x=\"{x}\" y=\"{baseline}\" font-family=\"sans-serif\" \
             font-size=\"{}\" font-weight=\"{weight}\" font-style=\"{style}\" \
             fill=\"#{red:02x}{green:02x}{blue:02x}\" xml:space=\"preserve\">{}</text>",
            look.font_px,
            escaped(text),
        ));
    }
    svg.push_str("</svg>");
    svg
}

/// `text` with the characters that are markup in XML escaped.
fn escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            c => escaped.push(c),
        }
    }
    escaped
}

/// The error for a heading that no font draws.
fn no_font() -> ImageError {
    ImageError::Diagram {
        name: "heading".into(),
        reason: "no font draws it".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::ImageKey;

    const CELL: (u16, u16) = (10, 20);

    fn look(rows: u8) -> LevelLook {
        LevelLook {
            rows,
            font_px: 30.0,
            bold: true,
            italic: false,
            color: [0x12, 0x80, 0xf0],
        }
    }

    fn drawn(text: &str, look: &LevelLook) -> (image::RgbaImage, Vec<u16>) {
        match draw_heading(text, look, CELL, &Fonts::fixture()) {
            Ok(Source::Text { image, breaks }) => (image, breaks),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_heading_image_is_as_tall_as_its_rows() {
        let (image, _) = drawn("Kiri", &look(2));
        assert_eq!(image.height(), 40);
        assert!(image.width() > 0);
    }

    #[test]
    fn a_heading_is_drawn_in_its_colour() {
        let look = look(2);
        let (image, _) = drawn("Kiri", &look);
        let [red, green, blue] = look.color;
        let colour = [red, green, blue, 255];
        assert!(image.pixels().any(|pixel| pixel.0 == colour));
        // The rest is transparent, for the heading's background to show.
        assert_eq!(image.get_pixel(0, 0).0[3], 0);
    }

    #[test]
    fn without_fonts_a_heading_has_no_image() {
        let drawn = draw_heading("Kiri", &look(2), CELL, &Fonts::none());
        assert_eq!(drawn, Err(no_font()));
    }

    #[test]
    fn the_same_text_at_two_levels_has_two_keys() {
        let (one, two) = (ImageKey::heading(1, "Kiri"), ImageKey::heading(2, "Kiri"));
        assert_ne!(one, two);
        assert_ne!(one, one.clone().in_document(1));
        assert_eq!(one.clone().in_document(1).document(), 1);
        assert_eq!(one.to_string(), "heading \"Kiri\"");
    }

    #[test]
    fn markup_in_heading_text_is_escaped() {
        let (image, breaks) = drawn("a < b & c", &look(1));
        assert!(image.pixels().any(|pixel| pixel.0[3] > 0));
        assert_eq!(breaks.len(), 4);
    }

    #[test]
    fn each_word_starts_on_a_whole_cell() {
        let (image, breaks) = drawn("One two three", &look(2));
        assert_eq!(breaks.len(), 2);
        assert!(breaks[0] > 0 && breaks[0] < breaks[1], "{breaks:?}");
        assert_eq!(image.width() % u32::from(CELL.0), 0);
        let columns = image.width() / u32::from(CELL.0);
        assert!(u32::from(breaks[1]) < columns, "{breaks:?} of {columns}");
    }

    #[test]
    fn heading_words_split_on_whitespace() {
        assert_eq!(heading_words(" One\ttwo  three\n"), ["One", "two", "three"]);
    }
}
