//! Headings drawn as images of their text, for a terminal that draws
//! images but cannot draw large text: each word starts on a whole cell, so
//! that a heading too long for the room can show the left part of its one
//! image, up to the last word that fits.

use crate::images::svg::{Fonts, Svg, decode_svg};
use crate::typeset::images::{ImageError, Source};
use crate::typeset::model::{Image, Inline};

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
            | Inline::Formula { written: inner, .. }
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
    // How far the ink of each word reaches from where the word starts:
    // how far before it, as an italic letter can lean, and how far after.
    let mut reaches = Vec::with_capacity(words.len());
    for word in words {
        let text = Drawn::new(0.0, word);
        let edges = svg(1, &[text], &[])?.text_edges();
        reaches.push(edges.map(|(left, right)| ((-left).max(0.0), right.max(0.0))));
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
        // Ink leaning before the word moves it on, so that it stays in
        // the word's own cells.
        let (lean, reach) = reach.unwrap_or((0.0, 0.0));
        let x = f32::from(start + before) * cell_width + lean;
        end = x + reach;
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
mod tests;
