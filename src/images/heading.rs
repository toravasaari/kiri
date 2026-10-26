//! Headings drawn as images of their text, for a terminal that draws
//! images but cannot draw large text: each word starts on a whole cell, so
//! that a heading too long for the room can show the left part of its one
//! image, up to the last word that fits.

use super::svg::{Fonts, Svg, decode_svg};
use super::{ImageError, Source};
use crate::model::{Image, Inline};

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
    /// How inline code in the heading is drawn.
    pub code: CodeLook,
}

/// How inline code in a heading image is drawn, as the text layout draws
/// it in a heading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeLook {
    /// The colour of its text, as RGB.
    pub color: [u8; 3],
    /// The colour behind it, as RGB; `None` for the heading's own.
    pub background: Option<[u8; 3]>,
    /// Whether a space of its background pads it on each side.
    pub padding: bool,
}

/// A word of a heading, as its image draws it from a whole cell.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeadingWord {
    pub text: String,
    /// The inline code the whole word is in, numbered from 0 in the
    /// heading, so that the words of one code span share a background;
    /// `None` for heading text.
    pub code: Option<u16>,
    /// Whether the word is emphasised, and so in italics.
    pub italic: bool,
    /// Whether the word follows the one before it without a space, as
    /// text right after inline code does: the two stay on one line.
    pub glued: bool,
}

/// The words of the heading text `text`, split on whitespace, none of them
/// code or emphasised.
pub fn plain_words(text: &str) -> Vec<HeadingWord> {
    (text.split_whitespace())
        .map(|word| HeadingWord {
            text: word.to_owned(),
            code: None,
            italic: false,
            glued: false,
        })
        .collect()
}

/// The words of a heading with `content`, split on whitespace as its
/// plain text is and where it goes into or out of inline code or
/// emphasis, each marked with the inline code or the key it is in, which
/// are drawn alike, and with whether it is emphasised. Other inline
/// styles are drawn as heading text.
pub fn heading_words(content: &[Inline]) -> Vec<HeadingWord> {
    let mut marked = Vec::new();
    mark(content, Marks::default(), &mut 0, &mut marked);
    let mut words: Vec<HeadingWord> = Vec::new();
    // Whether the next character starts a word after a space.
    let mut new = true;
    for (c, marks) in marked {
        if c.is_whitespace() {
            new = true;
            continue;
        }
        match words.last_mut() {
            Some(word) if !new && word.code == marks.code && word.italic == marks.italic => {
                word.text.push(c);
            }
            // Text going into or out of code or emphasis is a word of its
            // own, glued to the one before it.
            last => {
                let glued = !new && last.is_some();
                words.push(HeadingWord {
                    text: c.to_string(),
                    code: marks.code,
                    italic: marks.italic,
                    glued,
                });
            }
        }
        new = false;
    }
    words
}

/// What a character of a heading is in: the inline code or key, and
/// emphasis.
#[derive(Debug, Clone, Copy, Default)]
struct Marks {
    code: Option<u16>,
    italic: bool,
}

/// Appends the characters of the plain text of `content`, as
/// [`Inline::plain_text`] has it, to `out`, each with what it is in: the
/// marks around it, or the next of `spans` for an inline code or a key of
/// its own.
fn mark(content: &[Inline], marks: Marks, spans: &mut u16, out: &mut Vec<(char, Marks)>) {
    let text = |out: &mut Vec<_>, text: &str, marks| out.extend(text.chars().map(|c| (c, marks)));
    for inline in content {
        match inline {
            // Code inside a key is part of the key.
            Inline::Code(code) if marks.code.is_some() => text(out, code, marks),
            Inline::Code(code) => {
                let code_marks = Marks {
                    code: Some(*spans),
                    ..marks
                };
                text(out, code, code_marks);
                *spans += 1;
            }
            // A key is drawn as inline code is.
            Inline::Kbd(inner) if marks.code.is_none() => {
                let key = Marks {
                    code: Some(*spans),
                    ..marks
                };
                *spans += 1;
                mark(inner, key, spans, out);
            }
            Inline::Text(plain) | Inline::Html(plain) => text(out, plain, marks),
            Inline::SoftBreak | Inline::LineBreak => text(out, " ", marks),
            Inline::Emphasis(inner) => {
                let italic = Marks {
                    italic: true,
                    ..marks
                };
                mark(inner, italic, spans, out);
            }
            Inline::Strong(inner)
            | Inline::Strikethrough(inner)
            | Inline::Kbd(inner)
            | Inline::Mark(inner)
            | Inline::Subscript(inner)
            | Inline::Superscript(inner)
            | Inline::Link { content: inner, .. }
            | Inline::Image(Image { alt: inner, .. }) => mark(inner, marks, spans, out),
        }
    }
}

/// `text` drawn as `look` says with `fonts`, in cells of `cell` pixels:
/// one image `look.rows` cells high and a whole number of cells wide,
/// each word starting on a whole cell, with the columns the words after
/// the first start on and those each word's ink ends before. An error
/// when no font draws the text.
pub fn draw_heading(
    words: &[HeadingWord],
    look: &LevelLook,
    cell: (u16, u16),
    fonts: &Fonts,
) -> Result<Source, ImageError> {
    let cell_width = f32::from(cell.0);
    let height = u32::from(look.rows) * u32::from(cell.1);
    // The text sits on the bottom row as a sized heading's does, with
    // room below the baseline for the descenders.
    let baseline = height as f32 - DESCENT * look.font_px;
    let svg = |width: u32, texts: &[Drawn], backgrounds: &[(f32, f32)]| {
        let svg = heading_svg(texts, backgrounds, look, (width, height), baseline);
        decode_svg(svg.as_bytes(), u64::MAX, fonts)
    };
    // How far the ink of each word reaches from where the word starts.
    let mut reaches = Vec::with_capacity(words.len());
    for word in words {
        let text = Drawn::new(0.0, word);
        let edges = svg(1, &[text], &[])?.text_edges();
        reaches.push(edges.map(|(_, right)| right.max(0.0)));
    }
    if reaches.iter().all(Option::is_none) {
        return Err(no_font());
    }
    let space = space_width(&svg, false).unwrap_or(look.font_px * SPACE);
    // Inline code has a space of its own font before and after it, on its
    // background, as the text layout pads it, in whole cells.
    let padding = match look.code.padding {
        true => {
            let code_space = space_width(&svg, true).unwrap_or(look.font_px * SPACE);
            (code_space / cell_width).ceil() as u16
        }
        false => 0,
    };
    let joined = |a: usize, b: usize| words[a].code.is_some() && words[a].code == words[b].code;
    let mut starts: Vec<u16> = Vec::with_capacity(words.len());
    let mut ends: Vec<u16> = Vec::with_capacity(words.len());
    let mut texts = Vec::with_capacity(words.len());
    let mut end = 0.0_f32;
    for (index, (word, reach)) in words.iter().zip(&reaches).enumerate() {
        let code = word.code.is_some();
        let before = match code && (index == 0 || !joined(index - 1, index)) {
            true => padding,
            false => 0,
        };
        let after = match code && (index + 1 == words.len() || !joined(index, index + 1)) {
            true => padding,
            false => 0,
        };
        let start = match index {
            0 => 0,
            // Right after the word it is glued to.
            _ if word.glued => (end / cell_width).ceil() as u16,
            _ => ((end + space) / cell_width).ceil() as u16,
        };
        let x = f32::from(start + before) * cell_width;
        end = x + reach.unwrap_or(0.0);
        // A word without ink still covers a cell, so that a line it ends
        // shows something.
        let last = (((end / cell_width).ceil() as u16) + after).max(start + 1);
        if after > 0 {
            end = f32::from(last) * cell_width;
        }
        starts.push(start);
        ends.push(last);
        texts.push(Drawn::new(x, word));
    }
    // The background of each run of words in one inline code.
    let mut backgrounds = Vec::new();
    if look.code.background.is_some() {
        for index in 0..words.len() {
            let first = index == 0 || !joined(index - 1, index);
            if words[index].code.is_none() {
                continue;
            }
            let right = f32::from(ends[index]) * cell_width;
            match backgrounds.last_mut() {
                Some((_, end)) if !first => *end = right,
                _ => backgrounds.push((f32::from(starts[index]) * cell_width, right)),
            }
        }
    }
    let columns = u32::from(*ends.last().unwrap_or(&1));
    let width = columns * u32::from(cell.0);
    let image = svg(width, &texts, &backgrounds)?.render(width, height);
    Ok(Source::Text {
        image,
        breaks: starts.into_iter().skip(1).collect(),
        ends,
    })
}

/// A word drawn at `x`: heading text, or inline code, and in italics or
/// not besides as the heading is.
struct Drawn<'a> {
    x: f32,
    text: &'a str,
    code: bool,
    italic: bool,
}

impl<'a> Drawn<'a> {
    fn new(x: f32, word: &'a HeadingWord) -> Self {
        Drawn {
            x,
            text: &word.text,
            code: word.code.is_some(),
            italic: word.italic,
        }
    }
}

/// How far below the baseline the text reaches, in ems: the descent of
/// the usual sans fonts.
const DESCENT: f32 = 0.24;

/// The width of a space in ems, when no font tells.
const SPACE: f32 = 0.3;

/// The width of a space in the font `svg` draws heading text with, or
/// inline code with `code`: how much farther `x x` reaches than `xx`.
fn space_width(
    svg: &impl Fn(u32, &[Drawn], &[(f32, f32)]) -> Result<Svg, ImageError>,
    code: bool,
) -> Option<f32> {
    let reach = |text| {
        let text = Drawn {
            x: 0.0,
            text,
            code,
            italic: false,
        };
        Some(svg(1, &[text], &[]).ok()?.text_edges()?.1)
    };
    Some(reach("x x")? - reach("xx")?).filter(|&width| width > 0.0)
}

/// An SVG `size` pixels large with each of `texts` at its x and on
/// `baseline`, drawn as `look` says, over the background of inline code
/// from the left to the right of each of `backgrounds`, all the way down.
fn heading_svg(
    texts: &[Drawn],
    backgrounds: &[(f32, f32)],
    look: &LevelLook,
    size: (u32, u32),
    baseline: f32,
) -> String {
    let hex = |[red, green, blue]: [u8; 3]| format!("#{red:02x}{green:02x}{blue:02x}");
    let weight = if look.bold { "bold" } else { "normal" };
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">",
        size.0, size.1
    );
    if let Some(background) = look.code.background {
        for (left, right) in backgrounds {
            svg.push_str(&format!(
                "<rect x=\"{left}\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                right - left,
                size.1,
                hex(background),
            ));
        }
    }
    for text in texts {
        let (family, color) = match text.code {
            true => ("monospace", look.code.color),
            false => ("sans-serif", look.color),
        };
        let style = if look.italic || text.italic {
            "italic"
        } else {
            "normal"
        };
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{baseline}\" font-family=\"{family}\" \
             font-size=\"{}\" font-weight=\"{weight}\" font-style=\"{style}\" \
             fill=\"{}\" xml:space=\"preserve\">{}</text>",
            text.x,
            look.font_px,
            hex(color),
            escaped(text.text),
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
            code: CodeLook {
                color: [0xe0, 0x20, 0xe0],
                background: Some([0x30, 0x30, 0x30]),
                padding: true,
            },
        }
    }

    fn drawn(text: &str, look: &LevelLook) -> (image::RgbaImage, Vec<u16>) {
        match draw_heading(&plain_words(text), look, CELL, &Fonts::fixture()) {
            Ok(Source::Text { image, breaks, .. }) => (image, breaks),
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
        let drawn = draw_heading(&plain_words("Kiri"), &look(2), CELL, &Fonts::none());
        assert_eq!(drawn, Err(no_font()));
    }

    #[test]
    fn the_same_text_at_two_levels_has_two_keys() {
        let (one, two) = (
            ImageKey::heading(1, "Kiri", CELL),
            ImageKey::heading(2, "Kiri", CELL),
        );
        assert_ne!(one, two);
        assert_ne!(one, one.clone().in_document(1));
        assert_eq!(one.clone().in_document(1).document(), 1);
        assert_eq!(one.to_string(), "heading \"Kiri\"");
        // Drawn for other cells, its words start elsewhere.
        assert_ne!(one, ImageKey::heading(1, "Kiri", (8, 16)));
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
    fn each_word_ends_before_the_next_starts() {
        let drawn = draw_heading(
            &plain_words("One two three"),
            &look(2),
            CELL,
            &Fonts::fixture(),
        );
        let Ok(Source::Text {
            image,
            breaks,
            ends,
        }) = drawn
        else {
            panic!("{drawn:?}");
        };
        let columns = (image.width() / u32::from(CELL.0)) as u16;
        assert_eq!(ends.len(), 3, "{ends:?}");
        let starts = [0, breaks[0], breaks[1]];
        for (word, (&start, &end)) in starts.iter().zip(&ends).enumerate() {
            assert!(start < end, "word {word}: {starts:?} {ends:?}");
        }
        assert!(ends[0] <= breaks[0] && ends[1] <= breaks[1], "{ends:?}");
        assert_eq!(ends[2], columns);
    }

    #[test]
    fn a_word_without_ink_still_covers_a_cell() {
        // A zero-width space is a word of its own, and draws nothing.
        let drawn = draw_heading(
            &plain_words("Kiri \u{200B}"),
            &look(2),
            CELL,
            &Fonts::fixture(),
        );
        let Ok(Source::Text {
            image,
            breaks,
            ends,
        }) = drawn
        else {
            panic!("{drawn:?}");
        };
        assert_eq!(ends.len(), 2, "{ends:?}");
        assert!(ends[1] > breaks[0], "{breaks:?} {ends:?}");
        // The image reaches as far as that cell.
        assert_eq!(image.width(), u32::from(ends[1]) * u32::from(CELL.0));
    }

    /// `words` as heading words, each with the code span it is in.
    fn words(words: &[(&str, Option<u16>)]) -> Vec<HeadingWord> {
        (words.iter())
            .map(|&(text, code)| HeadingWord {
                text: text.to_owned(),
                code,
                italic: false,
                glued: false,
            })
            .collect()
    }

    /// `words` drawn as `look(2)` says, with its starts and ends.
    fn drawn_words(words: &[HeadingWord]) -> (image::RgbaImage, Vec<u16>, Vec<u16>) {
        let drawn = draw_heading(words, &look(2), CELL, &Fonts::fixture());
        match drawn {
            Ok(Source::Text {
                image,
                breaks,
                ends,
            }) => (image, std::iter::once(0).chain(breaks).collect(), ends),
            other => panic!("{other:?}"),
        }
    }

    /// The opaque colour `rgb`.
    fn opaque([red, green, blue]: [u8; 3]) -> [u8; 4] {
        [red, green, blue, 255]
    }

    #[test]
    fn a_code_word_is_drawn_on_its_background_in_its_colour() {
        let code = look(2).code;
        let (image, starts, ends) = drawn_words(&words(&[("Kiri", None), ("code", Some(0))]));
        let cells = |word: usize| {
            let x = u32::from(starts[word]) * 10..u32::from(ends[word]) * 10;
            let image = &image;
            x.flat_map(move |x| (0..40).map(move |y| image.get_pixel(x, y).0))
        };
        let background = opaque(code.background.unwrap());
        // On its background all over its cells, padding and all.
        let top = image.get_pixel(u32::from(starts[1]) * 10, 0).0;
        assert_eq!(top, background);
        assert!(cells(1).any(|pixel| pixel == opaque(code.color)));
        // The heading text on none.
        assert!(!cells(0).any(|pixel| pixel == background));
        assert!(!cells(0).any(|pixel| pixel == opaque(code.color)));
    }

    #[test]
    fn code_is_padded_by_a_space_at_the_heading_size() {
        // Narrow cells, so that a space of the 30-pixel font takes more
        // than one.
        let cell = (4, 20);
        let words = words(&[("a", None), ("code", Some(0))]);
        let drawn = draw_heading(&words, &look(2), cell, &Fonts::fixture());
        let Ok(Source::Text { image, breaks, .. }) = drawn else {
            panic!("{drawn:?}");
        };
        let start = u32::from(breaks[0]) * 4;
        let ink = (start..image.width())
            .find(|&x| (0..40).any(|y| image.get_pixel(x, y).0 == opaque(look(2).code.color)));
        // DejaVu Sans, the fixture's only font, has a space of 0.32 em:
        // 9.5 pixels, three cells.
        assert!(ink.unwrap() >= start + 3 * 4, "{ink:?} from {start}");
    }

    #[test]
    fn code_without_padding_has_its_background_from_its_first_cell() {
        let mut look = look(2);
        let words = words(&[("a", None), ("code", Some(0))]);
        let drawn = |look: &LevelLook| match draw_heading(&words, look, CELL, &Fonts::fixture()) {
            Ok(Source::Text { image, breaks, .. }) => (image, breaks),
            other => panic!("{other:?}"),
        };
        let (padded, _) = drawn(&look);
        look.code.padding = false;
        let (image, breaks) = drawn(&look);
        let start = u32::from(breaks[0]) * 10;
        let background = opaque(look.code.background.unwrap());
        assert_eq!(image.get_pixel(start, 0).0, background);
        assert_eq!(image.get_pixel(image.width() - 1, 0).0, background);
        // Narrower by the padding on both sides.
        assert!(
            image.width() < padded.width(),
            "{} {}",
            image.width(),
            padded.width()
        );
    }

    #[test]
    fn a_code_span_over_two_words_has_no_gap_in_its_background() {
        let background = opaque(look(2).code.background.unwrap());
        let row = |image: &image::RgbaImage, x: std::ops::Range<u32>| {
            x.map(|x| image.get_pixel(x, 0).0).collect::<Vec<_>>()
        };
        let (image, starts, ends) = drawn_words(&words(&[("a", Some(0)), ("b", Some(0))]));
        let span = u32::from(starts[0]) * 10..u32::from(ends[1]) * 10;
        assert!(row(&image, span).iter().all(|&pixel| pixel == background));
        // Two spans of code have the page between them.
        let (image, _, ends) = drawn_words(&words(&[("a", Some(0)), ("b", Some(1))]));
        let gap = u32::from(ends[0]) * 10..u32::from(ends[0]) * 10 + 1;
        assert!(
            row(&image, gap).iter().all(|pixel| pixel[3] == 0),
            "{ends:?}"
        );
    }

    #[test]
    fn without_a_code_background_none_is_drawn() {
        let mut look = look(2);
        look.code.background = None;
        let words = words(&[("code", Some(0))]);
        let drawn = draw_heading(&words, &look, CELL, &Fonts::fixture());
        let Ok(Source::Text { image, .. }) = drawn else {
            panic!("{drawn:?}");
        };
        assert_eq!(image.get_pixel(0, 0).0[3], 0);
        assert!(
            image
                .pixels()
                .any(|pixel| pixel.0 == opaque(look.code.color))
        );
    }

    #[test]
    fn the_words_of_inline_code_are_marked_with_their_span() {
        let content = crate::parse::parse("# a `b c` *d* `e`f, `g`\n");
        let crate::model::Block::Heading { content, .. } = &content.blocks[0] else {
            panic!("a heading");
        };
        let marked: Vec<(String, Option<u16>, bool)> = (heading_words(content).into_iter())
            .map(|word| (word.text, word.code, word.glued))
            .collect();
        let expected = [
            ("a", None, false),
            ("b", Some(0), false),
            ("c", Some(0), false),
            ("d", None, false),
            // Text against code is a word of its own, glued to it.
            ("e", Some(1), false),
            ("f,", None, true),
            ("g", Some(2), false),
        ];
        let expected: Vec<(String, Option<u16>, bool)> = (expected.iter())
            .map(|&(text, code, glued)| (text.to_owned(), code, glued))
            .collect();
        assert_eq!(marked, expected);
    }

    #[test]
    fn a_glued_word_starts_where_the_one_before_it_ends() {
        let mut words = plain_words("a code ,");
        words[1].code = Some(0);
        words[2].glued = true;
        let (_, starts, ends) = drawn_words(&words);
        // No space between the code's background and the comma.
        assert_eq!(starts[2], ends[1], "{starts:?} {ends:?}");
    }

    #[test]
    fn a_key_is_marked_as_inline_code_is() {
        let content = crate::parse::parse("# Press <kbd>Ctrl `C`</kbd> `now`\n");
        let crate::model::Block::Heading { content, .. } = &content.blocks[0] else {
            panic!("a heading");
        };
        let marked: Vec<(String, Option<u16>)> = (heading_words(content).into_iter())
            .map(|word| (word.text, word.code))
            .collect();
        // Code inside the key is part of the key.
        let expected = [
            ("Press", None),
            ("Ctrl", Some(0)),
            ("C", Some(0)),
            ("now", Some(1)),
        ];
        let expected: Vec<(String, Option<u16>)> = (expected.iter())
            .map(|&(text, code)| (text.to_owned(), code))
            .collect();
        assert_eq!(marked, expected);
    }

    #[test]
    fn emphasised_words_are_marked_as_italic() {
        let marked = |source: &str| {
            let document = crate::parse::parse(source);
            let crate::model::Block::Heading { content, .. } = &document.blocks[0] else {
                panic!("a heading");
            };
            (heading_words(content).into_iter())
                .map(|word| (word.text, word.italic, word.glued))
                .collect::<Vec<_>>()
        };
        let expected = |words: &[(&str, bool, bool)]| {
            (words.iter())
                .map(|&(text, italic, glued)| (text.to_owned(), italic, glued))
                .collect::<Vec<_>>()
        };
        // Text going into or out of emphasis is a word of its own, glued
        // to the one before it; strong is upright.
        assert_eq!(
            marked("# a *b `c`* d*e* **f**\n"),
            expected(&[
                ("a", false, false),
                ("b", true, false),
                ("c", true, false),
                ("d", false, false),
                ("e", true, true),
                ("f", false, false),
            ])
        );
        assert_eq!(
            marked("# Why *this*?\n"),
            expected(&[
                ("Why", false, false),
                ("this", true, false),
                ("?", false, true),
            ])
        );
    }

    #[test]
    fn an_italic_word_is_drawn_in_italics() {
        let mut words = plain_words("Kiri code");
        words[1].italic = true;
        words[1].code = Some(0);
        let texts: Vec<Drawn> = words.iter().map(|word| Drawn::new(0.0, word)).collect();
        let svg = heading_svg(&texts, &[], &look(2), (100, 40), 30.0);
        let styles: Vec<&str> = (svg.split("font-style=\"").skip(1))
            .map(|rest| &rest[..rest.find('"').unwrap()])
            .collect();
        assert_eq!(styles, ["normal", "italic"], "{svg}");
    }

    #[test]
    fn heading_words_split_on_whitespace() {
        let words = plain_words(" One\ttwo  three\n");
        let texts: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();
        assert_eq!(texts, ["One", "two", "three"]);
    }
}
