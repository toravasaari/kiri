//! Document model shared by all input formats and backends.
//!
//! The model says what the document means, not how a terminal draws it.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// The source line of each block, by [block path](SourceLines).
    pub lines: SourceLines,
}

impl Document {
    /// A document of `blocks` that does not know where they came from.
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            blocks,
            lines: SourceLines::default(),
        }
    }
}

/// The lines of the source that the blocks of a document start on,
/// counting from 1, by the path of each block.
///
/// A path says where a block is in its document: the number of a top-level
/// block, then the numbers that lead from it to the block, one step into a
/// `<details>`, quote or alert block (the number of the block inside it), or
/// two steps into a list (the number of the item, then the number of the
/// block inside the item). So `[2, 0, 1]` is the second block of the first
/// item of the list that is the third block of the document. A block that
/// is not in the source, such as a note, has no line.
///
/// A document read from another format has a line for each block that
/// starts in its file: a CSV table and a file shown as code start on the
/// first. It takes no part in comparing documents: two documents with the same
/// blocks are equal wherever the blocks were written.
#[derive(Debug, Clone, Default)]
pub struct SourceLines(std::collections::BTreeMap<Vec<usize>, usize>);

impl SourceLines {
    /// Records that the block at `path` starts on `line`.
    pub fn insert(&mut self, path: Vec<usize>, line: usize) {
        self.0.insert(path, line);
    }

    /// The line the block at `path` starts on, if it is known.
    pub fn get(&self, path: &[usize]) -> Option<usize> {
        self.0.get(path).copied()
    }
}

impl PartialEq for SourceLines {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for SourceLines {}

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
    /// Text set low and small, as in H₂O: HTML's `<sub>`.
    Subscript(Vec<Inline>),
    /// Text set high and small, as in x²: HTML's `<sup>`.
    Superscript(Vec<Inline>),
    Link {
        url: String,
        content: Vec<Inline>,
    },
    Image(Image),
    /// A formula in the middle of text, as `$x^2$` writes one.
    Formula {
        /// The formula, as LaTeX.
        source: String,
        /// What the source reads as without math, as Markdown: what a
        /// backend that cannot draw formulas shows.
        written: Vec<Inline>,
    },
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

    /// The image that `content` consists of, as [`Inline::lone_image`]
    /// finds it, and where the link around it goes, if there is one.
    pub fn lone_linked_image(content: &[Inline]) -> Option<(&Image, Option<&str>)> {
        match content {
            [Inline::Link { url, content }] => {
                Self::lone_image(content).map(|image| (image, Some(url.as_str())))
            }
            _ => Self::lone_image(content).map(|image| (image, None)),
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
                | Inline::Subscript(content)
                | Inline::Superscript(content)
                | Inline::Link { content, .. }
                | Inline::Formula {
                    written: content, ..
                }
                | Inline::Image(Image { alt: content, .. }) => {
                    out.push_str(&Self::plain_text(content))
                }
                Inline::SoftBreak | Inline::LineBreak => out.push(' '),
            }
        }
        out
    }
}

impl Value {
    /// The deepest table front matter shows: the top table is level 1, a
    /// table in one of its cells level 2. Values nested deeper show as
    /// YAML-like text.
    pub const TABLE_LEVELS: usize = 3;

    /// A value that is not a list or a map as text: strings as written,
    /// null as nothing.
    pub fn scalar(&self) -> Option<&str> {
        match self {
            Value::String(text) | Value::Number(text) => Some(text),
            Value::Bool(true) => Some("true"),
            Value::Bool(false) => Some("false"),
            Value::Null => Some(""),
            Value::List(_) | Value::Map(_) => None,
        }
    }

    pub fn is_scalar(&self) -> bool {
        self.scalar().is_some()
    }

    /// A list or a map as YAML in block style: kiri's own rendering of its
    /// tree, whatever format the front matter was written in.
    pub fn yaml(&self) -> String {
        let mut out = String::new();
        write_yaml(self, 0, &mut out);
        out
    }
}

fn is_empty_collection(value: &Value) -> bool {
    match value {
        Value::Map(entries) => entries.is_empty(),
        Value::List(items) => items.is_empty(),
        _ => false,
    }
}

/// Writes a list or a map as YAML in block style, indented by `indent`
/// spaces: kiri's own rendering of its tree, whatever format the front
/// matter was written in.
fn write_yaml(value: &Value, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match value {
        Value::Map(entries) => {
            for (key, value) in entries {
                out.push_str(&format!("{pad}{key}:"));
                write_nested(value, indent, out);
            }
        }
        Value::List(items) => {
            for item in items {
                match item {
                    // A list or a map starts after the item's dash.
                    Value::Map(_) | Value::List(_) if !is_empty_collection(item) => {
                        write_item(item, indent, out);
                    }
                    item => {
                        out.push_str(&format!("{pad}-"));
                        write_nested(item, indent, out);
                    }
                }
            }
        }
        scalar => write_scalar(scalar, indent, out),
    }
}

/// Writes a list or a map that is an item of a list at `indent`, its
/// first line after the item's dash.
fn write_item(item: &Value, indent: usize, out: &mut String) {
    let mut nested = String::new();
    write_yaml(item, indent + 2, &mut nested);
    out.push_str(&format!(
        "{}- {}",
        " ".repeat(indent),
        &nested[indent + 2..]
    ));
}

/// Writes `value` after a key or a dash at `indent`: a scalar on the same
/// line, a list or a map indented under it. An empty list or map shows as
/// `[]` or `{}`.
fn write_nested(value: &Value, indent: usize, out: &mut String) {
    match value {
        Value::Map(entries) if entries.is_empty() => out.push_str(" {}\n"),
        Value::List(items) if items.is_empty() => out.push_str(" []\n"),
        Value::Map(_) | Value::List(_) => {
            out.push('\n');
            write_yaml(value, indent + 2, out);
        }
        scalar => write_scalar(scalar, indent + 2, out),
    }
}

/// Writes a scalar after `: ` or `- `, ending its line; null leaves the
/// line empty. A string of several lines is a literal block indented by
/// `indent`.
fn write_scalar(value: &Value, indent: usize, out: &mut String) {
    let text = value.scalar().unwrap_or_default();
    if text.trim_end_matches('\n').contains('\n') {
        out.push_str(" |\n");
        for line in text.lines() {
            out.push_str(&format!("{}{line}\n", " ".repeat(indent)));
        }
    } else if text.is_empty() {
        out.push('\n');
    } else {
        out.push_str(&format!(" {}\n", text.trim_end_matches('\n')));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(text: &str) -> Value {
        Value::String(text.into())
    }

    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Map(
            (entries.iter())
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn writes_maps_and_lists_in_block_style() {
        let value = map(&[
            ("opt", Value::Number("3".into())),
            ("lto", Value::Bool(true)),
            ("env", Value::List(vec![string("CI=1"), string("LANG=C")])),
            ("owner", Value::Null),
            (
                "stops",
                Value::List(vec![
                    map(&[
                        ("name", string("Harbour")),
                        ("km", Value::Number("0".into())),
                    ]),
                    Value::List(vec![string("a")]),
                ]),
            ),
            ("none", Value::List(Vec::new())),
            ("empty", map(&[])),
        ]);
        assert_eq!(
            value.yaml(),
            "opt: 3\n\
             lto: true\n\
             env:\n  - CI=1\n  - LANG=C\n\
             owner:\n\
             stops:\n  - name: Harbour\n    km: 0\n  - - a\n\
             none: []\n\
             empty: {}\n"
        );
    }

    #[test]
    fn a_string_of_several_lines_is_a_literal_block() {
        let value = map(&[(
            "notes",
            map(&[
                ("text", string("one\ntwo\n")),
                ("folded", string("one two\n")),
            ]),
        )]);
        assert_eq!(
            value.yaml(),
            "notes:\n  text: |\n    one\n    two\n  folded: one two\n"
        );
    }
}
