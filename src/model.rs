//! Document model shared by all input formats and backends.
//!
//! The model says what the document means, not how a terminal draws it.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
}

/// One of several files shown one after another, each under a header that
/// names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// The name the header gives the file, such as its path.
    pub name: String,
    pub document: Document,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Metadata at the very start of a file, as static site generators
    /// and agent skills write it.
    FrontMatter(FrontMatter),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    CodeBlock {
        language: Option<String>,
        code: String,
        /// The line of the source its opening fence or first line is on,
        /// counting from 1, for messages about it.
        line: usize,
    },
    BlockQuote(Vec<Block>),
    /// A GitHub alert: a quote that starts with `[!NOTE]` or one of the
    /// other kinds.
    Alert {
        kind: AlertKind,
        /// A title written after the kind, in place of the kind's name.
        title: Option<String>,
        blocks: Vec<Block>,
    },
    List(List),
    /// HTML's `<details>`: blocks a reader can show or hide, under a
    /// summary that is always shown.
    Details {
        summary: Vec<Inline>,
        blocks: Vec<Block>,
    },
    Table(Table),
    ThematicBreak,
    /// Raw HTML, shown as source until there is an HTML backend.
    Html(String),
    /// A note of kiri's own about the document rather than part of it,
    /// such as how many rows of a table were left out.
    Note(String),
    /// A message of kiri's own that something went wrong, such as why a
    /// file among several cannot be shown. It stands out where a note is
    /// played down.
    Error(String),
    /// A file shown as an image on its own, as large as fits the window
    /// and no larger than its natural size: an image file, or a diagram
    /// file drawn as one. Its pixels come with the file, keyed by its
    /// name, rather than from a URL in a document.
    ImageFile(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatter {
    /// `yaml`, `toml` or `json` from the delimiter, or the language
    /// written after it, as in `---php`.
    pub language: String,
    /// The text between the delimiter lines, each line ending in `\n`.
    pub source: String,
    /// What YAML, TOML or JSON source holds, or why it could not be read;
    /// `None` for other languages.
    pub value: Option<Result<Value, FrontMatterError>>,
}

/// Why front matter could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatterError {
    /// The line of the file the parser stopped at, counting from 1, when
    /// the parser tells.
    pub line: Option<usize>,
    /// The parser's message, without a position.
    pub message: String,
}

impl std::fmt::Display for FrontMatterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "front matter, line {line}: {}", self.message),
            None => write!(f, "front matter: {}", self.message),
        }
    }
}

/// A value in front matter, whatever format it was written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String(String),
    /// A number as its format gives it, such as `1.10` in YAML.
    Number(String),
    Bool(bool),
    Null,
    List(Vec<Value>),
    /// Keys and their values in the order the source gives them.
    Map(Vec<(String, Value)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl AlertKind {
    /// The title GitHub gives an alert of this kind.
    pub fn name(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
        }
    }
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
    /// Keyboard input, such as a key to press: HTML's `<kbd>`.
    Kbd(Vec<Inline>),
    /// Text marked as relevant, as a highlighter would: HTML's `<mark>`.
    Mark(Vec<Inline>),
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
    /// The size the document asks for, as HTML's `width` and `height`
    /// give it. `None` leaves it to the image.
    pub width: Option<Length>,
    pub height: Option<Length>,
}

/// A length as HTML gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Length {
    /// CSS pixels.
    Pixels(u32),
    /// A percentage of the width there is, as a browser takes it of the
    /// block around the image.
    Percent(u32),
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
                | Inline::Kbd(content)
                | Inline::Mark(content)
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
