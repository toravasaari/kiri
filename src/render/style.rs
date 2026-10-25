//! Styled text: the unit between layout and output.

use std::rc::Rc;

use super::block::BlockId;
use super::image::Placement;
use crate::color::{Color, Underline};
use crate::size::TextSize;
use crate::theme::Theme;

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
}

impl Style {
    pub fn fg(color: Option<Color>) -> Self {
        Self {
            fg: color,
            ..Self::default()
        }
    }

    /// The same style without the hyperlink and the wrapping, for comparing
    /// attributes only.
    pub fn attributes(&self) -> Self {
        Self {
            link: None,
            spaces: Spaces::Collapse,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
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
        Self { text, style }
    }
}

/// One row of an image shown as a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRow {
    pub placement: Rc<Placement>,
    /// Counted from 0 at the top of the image.
    pub row: u16,
}

/// The band that tells that a block's image is loading, which a line
/// draws over itself, so that it takes no row of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    /// How many cells wide it is.
    pub columns: u16,
    /// How many cells from the start of the line it starts.
    pub indent: usize,
    /// Over the bottom of the line, as on the last row of a block with no
    /// blank line after it; otherwise a little below the line's top, as on
    /// the blank line after a block.
    pub low: bool,
}

/// One output line. A line of a size other than the normal one is drawn
/// with kitty's text sizing protocol and occupies as many terminal rows as
/// its scale.
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
    pub size: TextSize,
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
            size: TextSize::NORMAL,
            block: None,
            band: None,
        }
    }

    pub fn blank() -> Self {
        Self::new(Vec::new())
    }

    /// A line of one image row.
    pub fn image(placement: Rc<Placement>, row: u16) -> Self {
        Self {
            image: Some(ImageRow { placement, row }),
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
}

/// Text in `style` played down: in `theme`'s muted colour, or dimmed when
/// the theme has none.
pub fn muted(theme: &Theme, style: Style) -> Style {
    match theme.muted {
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

/// Appends text, merging it into the last span when the style matches.
pub fn push(spans: &mut Vec<Span>, text: &str, style: &Style) {
    if text.is_empty() {
        return;
    }
    let span = Span::new(text, style.clone());
    match spans.last_mut() {
        Some(last) if last.style == span.style => last.text.push_str(&span.text),
        _ => spans.push(span),
    }
}
