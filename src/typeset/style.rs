//! Styled text: the unit between layout and output.

use std::ops::Range;
use std::rc::Rc;

use crate::typeset::reading::engine::RowLink;
use crate::typeset::reading::page::Overlay;

use crate::typeset::block::BlockId;
use crate::typeset::color::{Color, Underline};
use crate::typeset::images::ImageKey;
use crate::typeset::images::size::{Fit, Placement};
use crate::typeset::reading::engine::Parts;
use crate::typeset::size::TextSize;
use crate::typeset::theme::Theme;

/// How the wrapper treats spaces in a span, much like CSS `white-space`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Spaces {
    /// Runs collapse to one, and the line can break at any of them.
    #[default]
    Collapse,
    /// Kept as written; the line can break at any of them.
    Keep,
    /// Kept as written; the line breaks at them only when the text they
    /// join is longer than a line.
    KeepTogether,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: Underline,
    /// The underline's own colour; `None` draws it in the text colour.
    pub underline_color: Option<Color>,
    pub strike: bool,
    /// Foreground and background swapped.
    pub reverse: bool,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    /// Target of an OSC 8 hyperlink.
    pub link: Option<Rc<str>>,
    pub spaces: Spaces,
    /// The size the text is drawn at.
    pub size: TextSize,
}

impl Style {
    pub fn fg(color: Option<Color>) -> Self {
        Self {
            fg: color,
            ..Self::default()
        }
    }

    /// The same style without the hyperlink, the wrapping and the size, for
    /// comparing attributes only.
    pub fn attributes(&self) -> Self {
        Self {
            link: None,
            spaces: Spaces::Collapse,
            size: TextSize::NORMAL,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    /// An image drawn in the cells of the span, in place of its text.
    pub embed: Option<Rc<Embed>>,
}

/// An image drawn in the cells of a span, as an inline image or formula is
/// in running text: as wide as `placement.size.columns`, whatever the
/// span's text says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Embed {
    pub placement: Rc<Placement>,
    /// The image row on the line's first row; the line's next rows show the
    /// next ones.
    pub row: u16,
    /// How many of the image's rows the line shows: all of the line's rows,
    /// or 1 on each line of a split.
    pub rows: u16,
    /// How the image was fitted into the line, which fits it again when the
    /// line is shaped anew: its opaque rows, baseline and rows.
    pub fit: Fit,
    /// Whether the image is a formula, whose baseline lies on the text's,
    /// and not a picture, whose opaque bottom lies on the bottom of the
    /// text's cell unless it is tall.
    pub formula: bool,
}

impl Embed {
    /// An embed of `placement` showing `rows` rows of it from `row`, whose
    /// image is wholly opaque and has its baseline at its bottom.
    pub fn new(placement: Rc<Placement>, row: u16, rows: u16) -> Self {
        let size = placement.size;
        let fit = Fit {
            width: size.width,
            height: size.height,
            opaque: 0..size.height,
            columns: size.columns,
            rows: size.rows,
            baseline: size.height,
        };
        Self {
            placement,
            row,
            rows,
            fit,
            formula: false,
        }
    }
}

impl Span {
    /// Control characters are replaced, so a document cannot smuggle escape
    /// sequences to the terminal. Newlines stay: they mark hard line breaks
    /// for the wrapper, which consumes them.
    pub fn new(text: &str, style: Style) -> Self {
        let text = text
            .chars()
            .map(|c| match c {
                '\n' => '\n',
                '\t' => ' ',
                c if c.is_control() => char::REPLACEMENT_CHARACTER,
                c => c,
            })
            .collect();
        Self {
            text,
            style,
            embed: None,
        }
    }

    /// A span that is the cells of the image `embed`; `text` is what a
    /// search finds in it, sanitised as `new` does.
    pub fn embed(text: &str, style: Style, embed: Embed) -> Self {
        Self {
            embed: Some(Rc::new(embed)),
            ..Self::new(text, style)
        }
    }
}

#[cfg(test)]
impl Span {
    /// A span that is the cells of an image `columns` wide and one row
    /// high, with `text` for a search to find, in `style`.
    pub(crate) fn image(text: &str, columns: u16, style: Style) -> Span {
        use crate::typeset::images::size::Shows;
        let size = crate::typeset::images::size::Size {
            width: u32::from(columns) * 10,
            height: 20,
            columns,
            rows: 1,
        };
        let placement = Placement::block(ImageKey::from("e.png"), size, Shows::Image);
        let embed = Embed::new(Rc::new(placement), 0, 1);
        Span::embed(text, style, embed)
    }
}

/// One row of an image shown as a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRow {
    pub placement: Rc<Placement>,
    /// Counted from 0 at the top of the image.
    pub row: u16,
    /// The background of the block it lies in, such as an alert's tint,
    /// which shows through where the image is transparent; `None` for the
    /// page's.
    pub background: Option<Color>,
    /// The columns of the image the row shows: all of them, or a part,
    /// as a line of a heading too long for one line shows its words.
    pub columns: Range<u16>,
    /// The text the line of the image shows, such as a heading's words,
    /// which a search finds on its first row; `None` for every other
    /// image.
    pub text: Option<String>,
    /// The page of reading mode that the image is a part of, which draws
    /// it; `None` for an image from `Images`.
    pub page: Option<Rc<Parts>>,
    /// What is drawn over the part of the page, such as a search's marks,
    /// when the key says so.
    pub overlay: Option<Rc<Overlay>>,
    /// The hyperlinks over the cells of the row, as the page has them.
    pub links: Vec<RowLink>,
}

/// The band that tells that a block's image is loading, which a line
/// draws over itself, so that it takes no row of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Band {
    /// How many cells wide it is.
    pub columns: u16,
    /// How many cells from the start of the line it starts.
    pub indent: usize,
    /// Over the bottom of the line, as on the last row of a block with no
    /// blank line after it; otherwise a little below the line's top, as on
    /// the blank line after a block.
    pub low: bool,
    /// The image loading.
    pub key: ImageKey,
    /// How many lines above this one show the image, or what stands in
    /// for it: 0 when this line is the only one.
    pub above: usize,
}

/// One output line. Text of a size other than the normal one is drawn with
/// kitty's text sizing protocol, and the line occupies as many terminal rows
/// as the largest scale of its spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Normal-size cells repeated at the start of every row the line
    /// occupies, so an indent keeps its colour beside scaled text.
    pub lead: Vec<Span>,
    pub spans: Vec<Span>,
    /// A row of an image after the spans, which then hold only what goes
    /// before it, such as a quote bar or a list marker.
    pub image: Option<ImageRow>,
    /// Normal-size cells after the text on every row the line occupies, so
    /// a background reaches the edge when the scaled text cannot.
    pub trail: Vec<Span>,
    /// The block the line comes from, or none for the blank line between
    /// two blocks.
    pub block: Option<BlockId>,
    /// The band of a block above whose image is loading, drawn over the
    /// line.
    pub band: Option<Band>,
}

impl Line {
    pub fn new(spans: Vec<Span>) -> Self {
        Self {
            lead: Vec::new(),
            spans,
            image: None,
            trail: Vec::new(),
            block: None,
            band: None,
        }
    }

    pub fn blank() -> Self {
        Self::new(Vec::new())
    }

    /// A line of one image row.
    pub fn image(placement: Rc<Placement>, row: u16) -> Self {
        let columns = 0..placement.size.columns;
        Self {
            image: Some(ImageRow {
                placement,
                row,
                background: None,
                columns,
                text: None,
                page: None,
                overlay: None,
                links: Vec::new(),
            }),
            ..Self::blank()
        }
    }

    /// Nothing to show: no text and no image.
    pub fn is_blank(&self) -> bool {
        self.spans.is_empty() && self.image.is_none()
    }

    pub fn prepend(&mut self, span: Span) {
        self.spans.insert(0, span);
    }

    /// The rows the line occupies: as many as the largest scale of its
    /// text spans or the most rows of its embeds, and at least one.
    pub fn rows(&self) -> u8 {
        (self.spans.iter())
            .map(|span| match &span.embed {
                Some(embed) => u8::try_from(embed.rows).unwrap_or(u8::MAX),
                None => span.style.size.scale(),
            })
            .fold(1, u8::max)
    }

    /// Draws the whole line at the normal size.
    pub fn to_normal_size(&mut self) {
        for span in &mut self.spans {
            span.style.size = TextSize::NORMAL;
        }
    }
}

/// Text in `style` played down: in `theme`'s muted colour, or dimmed when
/// the theme has none.
pub fn muted(theme: &Theme, style: Style) -> Style {
    muted_in(theme.muted, style)
}

/// Text in `style` played down: in `muted`, or dimmed when there is none.
pub fn muted_in(muted: Option<Color>, style: Style) -> Style {
    match muted {
        Some(color) => Style {
            fg: Some(color),
            ..style
        },
        None => Style { dim: true, ..style },
    }
}

/// The text of `spans` one after another.
pub fn text(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// The `Rc` for a link to `url` that comes right after `spans`. When the
/// last span is a link to `url` too, as in `[a](u)**[b](u)**`, it is that
/// span's `Rc`: the pager joins a link across lines by its `Rc`
/// (`text_links`), so the two stay one link where a line breaks between
/// them, as the terminal shows them as one. Otherwise it is a new `Rc`.
pub fn link_to(spans: &[Span], url: &str) -> Rc<str> {
    let before = spans.last().and_then(|span| span.style.link.clone());
    before
        .filter(|before| **before == *url)
        .unwrap_or_else(|| Rc::from(url))
}

/// Appends text, merging it into the last span when the style matches.
pub fn push(spans: &mut Vec<Span>, text: &str, style: &Style) {
    if text.is_empty() {
        return;
    }
    let span = Span::new(text, style.clone());
    match spans.last_mut() {
        Some(last) if last.embed.is_none() && last.style == span.style => {
            last.text.push_str(&span.text)
        }
        _ => spans.push(span),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::images::size::{Shows, Size};

    /// A span of an image of `rows` rows that a line shows `shown` rows of,
    /// from its row `row`.
    fn image_span(row: u16, shown: u16, rows: u16) -> Span {
        let size = Size {
            width: 10,
            height: 20 * u32::from(rows),
            columns: 1,
            rows,
        };
        let placement = Placement::block(ImageKey::from("e.png"), size, Shows::Image);
        let embed = Embed::new(Rc::new(placement), row, shown);
        Span::embed("", Style::default(), embed)
    }

    #[test]
    fn a_line_has_as_many_rows_as_its_tallest_text_or_embed() {
        assert_eq!(Line::new(vec![Span::new("a", Style::default())]).rows(), 1);
        // One row of a taller image on a line of a split.
        assert_eq!(Line::new(vec![image_span(2, 1, 4)]).rows(), 1);
        // A line of two rows shows two of its image's.
        let line = Line::new(vec![Span::new("a", Style::default()), image_span(0, 2, 2)]);
        assert_eq!(line.rows(), 2);
        let scaled = Style {
            size: TextSize::new(3).unwrap(),
            ..Style::default()
        };
        assert_eq!(
            Line::new(vec![Span::new("a", scaled), image_span(0, 2, 2)]).rows(),
            3
        );
    }
}
