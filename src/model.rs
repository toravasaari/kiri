//! Document model shared by all input formats and backends.
//!
//! The model says what the document means, not how a terminal draws it.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    BlockQuote(Vec<Block>),
    List(List),
    Table(Table),
    ThematicBreak,
    /// Raw HTML, shown as source until there is an HTML backend.
    Html(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    /// First number of an ordered list; `None` for a bullet list.
    pub start: Option<u64>,
    /// Tight lists have no blank lines between items.
    pub tight: bool,
    pub items: Vec<ListItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    /// `Some` for task list items: whether the box is ticked.
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub alignments: Vec<Alignment>,
    pub header: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Code(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Strikethrough(Vec<Inline>),
    Link {
        url: String,
        content: Vec<Inline>,
    },
    Image(Image),
    /// A line break in the source that renders as a space.
    SoftBreak,
    LineBreak,
    Html(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub url: String,
    pub alt: Vec<Inline>,
}

impl Inline {
    /// The image that `content`, a paragraph, consists of: an image alone,
    /// or alone inside a link. A backend that can draw images shows such a
    /// paragraph as the image itself; images anywhere else are part of the
    /// text.
    pub fn lone_image(content: &[Inline]) -> Option<&Image> {
        match content {
            [Inline::Image(image)] => Some(image),
            [Inline::Link { content, .. }] => match content.as_slice() {
                [Inline::Image(image)] => Some(image),
                _ => None,
            },
            _ => None,
        }
    }

    /// The inline's text without any styling, as alt text or for measuring.
    pub fn plain_text(inlines: &[Inline]) -> String {
        let mut out = String::new();
        for inline in inlines {
            match inline {
                Inline::Text(text) | Inline::Code(text) | Inline::Html(text) => out.push_str(text),
                Inline::Emphasis(content)
                | Inline::Strong(content)
                | Inline::Strikethrough(content)
                | Inline::Link { content, .. }
                | Inline::Image(Image { alt: content, .. }) => {
                    out.push_str(&Self::plain_text(content))
                }
                Inline::SoftBreak | Inline::LineBreak => out.push(' '),
            }
        }
        out
    }
}
