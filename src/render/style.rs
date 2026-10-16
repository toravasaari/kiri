//! Styled text: the unit between layout and output.

use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// An entry of the terminal's 256-colour palette; 0-15 follow its theme.
    Palette(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: Underline,
    pub strike: bool,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    /// Target of an OSC 8 hyperlink.
    pub link: Option<Rc<str>>,
}

impl Style {
    pub fn fg(color: Option<Color>) -> Self {
        Self {
            fg: color,
            ..Self::default()
        }
    }

    /// The same style without the hyperlink, for comparing attributes only.
    pub fn attributes(&self) -> Self {
        Self {
            link: None,
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

/// One output line. A line with `scale` above 1 is drawn with kitty's text
/// sizing protocol and occupies that many terminal rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Normal-size cells repeated at the start of every row the line
    /// occupies, so an indent keeps its colour beside scaled text.
    pub lead: Vec<Span>,
    pub spans: Vec<Span>,
    /// Normal-size cells after the text on every row the line occupies, so
    /// a background reaches the edge when the scaled text cannot.
    pub trail: Vec<Span>,
    pub scale: u8,
}

impl Line {
    pub fn new(spans: Vec<Span>) -> Self {
        Self {
            lead: Vec::new(),
            spans,
            trail: Vec::new(),
            scale: 1,
        }
    }

    pub fn blank() -> Self {
        Self::new(Vec::new())
    }

    pub fn prepend(&mut self, span: Span) {
        self.spans.insert(0, span);
    }
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
