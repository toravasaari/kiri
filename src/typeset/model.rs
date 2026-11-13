//! Document model shared by all input formats and backends.
//!
//! The model says what the document means, not how a terminal draws it.
//!
//! Three things are told of a block, and they are different things. Its
//! path in the document is worked out by walking the blocks and is not
//! stored. Its hash comes from the derived `Hash` and is not stored either;
//! it is stable within one build of kiri. A hash that has to outlast a
//! build needs a hand-written `Hash`, as the derived one follows the order
//! of fields and variants. Where it was written in
//! the source is recorded by the parser in the document's [`SourceMap`],
//! outside the blocks, and counts for neither equality nor the hash.
//! Spans inside a block have none of the three.
//!
//! The hash is not stored, but [`Hashes`] works it out in one go for every
//! path of a document, so that whoever holds a version of the document
//! can keep the table beside it.

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// Where each block was written, by [block path](SourceMap).
    pub places: SourceMap,
}

impl Document {
    /// A document of `blocks` that does not know where they came from.
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            blocks,
            places: SourceMap::default(),
        }
    }

    /// Every block with its path, each before the blocks inside it.
    pub fn blocks(&self) -> Vec<(Vec<usize>, &Block)> {
        fn walk<'a>(block: &'a Block, path: Vec<usize>, out: &mut Vec<(Vec<usize>, &'a Block)>) {
            for (steps, child) in block.children() {
                let path = steps.after(&path);
                out.push((path.clone(), child));
                walk(child, path, out);
            }
        }
        let mut out = Vec::new();
        for (index, block) in self.blocks.iter().enumerate() {
            out.push((vec![index], block));
            walk(block, vec![index], &mut out);
        }
        out
    }
}

/// Where in the source each block of a document was written, by the
/// path of the block.
///
/// A path says where a block is in its document: the number of a top-level
/// block, then the numbers that lead from it to the block, one step into a
/// `<details>`, quote or alert block (the number of the block inside it), or
/// two steps into a list (the number of the item, then the number of the
/// block inside the item). So `[2, 0, 1]` is the second block of the first
/// item of the list that is the third block of the document. A block that
/// is not in the source, such as a note, has no place.
///
/// A document read from another format has a place for each block that
/// starts in its file: a CSV table and a file shown as code take the whole
/// file. The map takes no part in comparing or hashing documents: two
/// documents with the same blocks are equal wherever the blocks were
/// written.
///
/// A place is all the map holds for a block today. A map from the text of
/// a block to the bytes it came from, for an editor that has to put the
/// cursor on a word, would be another field of [`Place`]; nothing needs it
/// yet.
#[derive(Debug, Clone, Default)]
pub struct SourceMap(std::collections::BTreeMap<Vec<usize>, Place>);

/// Where one block was written in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The line the block itself starts on, counting from 1. For an
    /// [`Extent::Inside`] block the bytes start at the larger piece, which
    /// can start on an earlier line.
    pub line: usize,
    /// The bytes of the source it was read from, without the line break
    /// that ends them, unless the block is a whole file.
    pub bytes: std::ops::Range<usize>,
    /// How well the bytes say what the block is.
    pub extent: Extent,
}

impl Place {
    /// The place of a block that is a whole file of `len` bytes, line break
    /// at its end and all.
    pub fn file(len: usize) -> Self {
        Self {
            line: 1,
            bytes: 0..len,
            extent: Extent::Exact,
        }
    }
}

/// How a [`Place`]'s bytes relate to its block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extent {
    /// The bytes are the source of this block and nothing else, except
    /// that the bytes of a `<details>` hold the blocks inside it.
    Exact,
    /// The block was read from part of a larger piece of source, such as
    /// one HTML block that holds several blocks, or a paragraph that
    /// display math splits: the bytes are the whole piece.
    Inside,
}

impl SourceMap {
    /// Records the place of the block at `path`.
    pub fn insert(&mut self, path: Vec<usize>, place: Place) {
        self.0.insert(path, place);
    }

    /// Where the block at `path` was written, if it is known.
    pub fn get(&self, path: &[usize]) -> Option<&Place> {
        self.0.get(path)
    }

    /// The line the block at `path` starts on, if it is known.
    pub fn line(&self, path: &[usize]) -> Option<usize> {
        self.get(path).map(|place| place.line)
    }

    /// The paths that have a place, in order.
    #[cfg(test)]
    pub(crate) fn paths(&self) -> impl Iterator<Item = &Vec<usize>> {
        self.0.keys()
    }
}

impl PartialEq for SourceMap {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for SourceMap {}

/// Hashes nothing, as it takes no part in comparing documents: what is
/// equal hashes alike.
impl std::hash::Hash for SourceMap {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// The hash of every block of a document, and of every item of its lists,
/// by path. The hash of a block covers everything inside it, so when a block
/// has the hash it had in another version, nothing inside it has changed.
///
/// It is made by [`Document::hashes`] for one version of a document and not
/// kept in the document, which anyone can change through its public
/// `blocks`: whoever owns a version keeps the table beside it. The values
/// are those of the derived `Hash` with the standard hasher: they are the
/// same for equal blocks wherever they are, and stable within one build of
/// kiri only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hashes {
    blocks: std::collections::BTreeMap<Vec<usize>, u64>,
    /// By the path of the list and the number of the item.
    items: std::collections::BTreeMap<(Vec<usize>, usize), u64>,
}

impl Hashes {
    /// The hash of the block at `path`.
    pub fn block(&self, path: &[usize]) -> Option<u64> {
        self.blocks.get(path).copied()
    }

    /// The hash of item number `item` of the list at `list`.
    pub fn item(&self, list: &[usize], item: usize) -> Option<u64> {
        self.items.get(&(list.to_vec(), item)).copied()
    }
}

#[cfg(test)]
thread_local! {
    /// How many times [`Document::hashes`] ran on this thread, which the
    /// tests of who calls it count.
    static HASHES_MADE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times [`Document::hashes`] ran on this thread.
#[cfg(test)]
pub(crate) fn hashes_made() -> usize {
    HASHES_MADE.with(std::cell::Cell::get)
}

impl Document {
    /// The hash of every block and list item of the document, by path.
    pub fn hashes(&self) -> Hashes {
        #[cfg(test)]
        HASHES_MADE.with(|made| made.set(made.get() + 1));
        fn hash_of(value: &impl std::hash::Hash) -> u64 {
            use std::hash::Hasher;
            let mut hasher = std::hash::DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        }
        let mut hashes = Hashes::default();
        for (path, block) in self.blocks() {
            if let Block::List(list) = block {
                for (index, item) in list.items.iter().enumerate() {
                    hashes.items.insert((path.clone(), index), hash_of(item));
                }
            }
            hashes.blocks.insert(path, hash_of(block));
        }
        hashes
    }
}

/// One of several files shown one after another, each under a header that
/// names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct File {
    /// The name the header gives the file, such as its path.
    pub name: String,
    pub document: Document,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
        /// Whether it is a whole file shown as text, not a block inside a
        /// document: it is then drawn bare, without the box that sets a
        /// block off from the text around it.
        whole_file: bool,
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

/// What a block holds of other blocks.
#[derive(Debug, Clone, Copy)]
pub enum Contents<'a> {
    None,
    /// A quote, an alert or a `<details>`: one path step to each block.
    Blocks(&'a [Block]),
    /// A list: two path steps, the item and the block inside it.
    Items(&'a [ListItem]),
}

/// The path steps from a block to a block inside it, as the
/// [path rule](SourceMap) counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steps {
    One(usize),
    Two(usize, usize),
}

impl Steps {
    /// The path `path` followed by these steps.
    pub fn after(self, path: &[usize]) -> Vec<usize> {
        let mut joined = path.to_vec();
        match self {
            Self::One(a) => joined.push(a),
            Self::Two(a, b) => joined.extend([a, b]),
        }
        joined
    }
}

impl Block {
    /// What this block holds of other blocks. This is the only place that
    /// says which kinds of block hold others, so a new kind does not
    /// compile until it is decided here.
    pub fn contents(&self) -> Contents<'_> {
        match self {
            Self::BlockQuote(blocks)
            | Self::Alert { blocks, .. }
            | Self::Details { blocks, .. } => Contents::Blocks(blocks),
            Self::List(list) => Contents::Items(&list.items),
            Self::FrontMatter(_)
            | Self::Heading { .. }
            | Self::Paragraph(_)
            | Self::CodeBlock { .. }
            | Self::Table(_)
            | Self::ThematicBreak
            | Self::Html(_)
            | Self::Note(_)
            | Self::Error(_)
            | Self::ImageFile(_) => Contents::None,
        }
    }

    /// Each block directly inside this one, with the path steps from this
    /// block to it.
    pub fn children(&self) -> impl Iterator<Item = (Steps, &Block)> {
        let (blocks, items): (&[Block], &[ListItem]) = match self.contents() {
            Contents::None => (&[], &[]),
            Contents::Blocks(blocks) => (blocks, &[]),
            Contents::Items(items) => (&[], items),
        };
        let direct = blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (Steps::One(index), block));
        let in_items = items.iter().enumerate().flat_map(|(item, list_item)| {
            (list_item.blocks.iter().enumerate())
                .map(move |(index, block)| (Steps::Two(item, index), block))
        });
        direct.chain(in_items)
    }
}

#[cfg(test)]
impl Block {
    /// What kind of block this is, in words, for the names of tests' failures.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Block::FrontMatter(_) => "front matter",
            Block::Heading { .. } => "heading",
            Block::Paragraph(_) => "paragraph",
            Block::CodeBlock { .. } => "code block",
            Block::BlockQuote(_) => "quote",
            Block::Alert { .. } => "alert",
            Block::List(_) => "list",
            Block::Details { .. } => "details",
            Block::Table(_) => "table",
            Block::ThematicBreak => "rule",
            Block::Html(_) => "html",
            Block::Note(_) => "note",
            Block::Error(_) => "error",
            Block::ImageFile(_) => "image file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct List {
    /// First number of an ordered list; `None` for a bullet list.
    pub start: Option<u64>,
    /// Tight lists have no blank lines between items.
    pub tight: bool,
    pub items: Vec<ListItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ListItem {
    /// `Some` for task list items: whether the box is ticked.
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Table {
    pub alignments: Vec<Alignment>,
    pub header: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Image {
    pub url: String,
    pub alt: Vec<Inline>,
    /// The size the document asks for, as HTML's `width` and `height`
    /// give it. `None` leaves it to the image.
    pub width: Option<Length>,
    pub height: Option<Length>,
}

/// A length as HTML gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    use std::hash::{DefaultHasher, Hash, Hasher};

    use super::*;

    fn hash_of(document: &Document) -> u64 {
        let mut hasher = DefaultHasher::new();
        document.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn a_document_hashes_by_its_blocks_and_not_by_its_line_table() {
        let blocks = || vec![Block::Paragraph(vec![Inline::Text("one".into())])];
        let plain = Document::new(blocks());
        let mut placed = Document::new(blocks());
        placed.places.insert(vec![0], Place::file(7));
        assert_eq!(hash_of(&plain), hash_of(&placed));
        let other = Document::new(vec![Block::Paragraph(vec![Inline::Text("two".into())])]);
        assert_ne!(hash_of(&plain), hash_of(&other));
    }

    fn paragraph(text: &str) -> Block {
        Block::Paragraph(vec![Inline::Text(text.into())])
    }

    fn hash_value<T: Hash>(value: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    fn fixture_documents() -> Vec<(std::path::PathBuf, Document)> {
        crate::testing::documents()
            .into_iter()
            .map(|path| {
                let source = std::fs::read_to_string(&path).unwrap();
                (path, crate::typeset::parse::parse(&source))
            })
            .collect()
    }

    #[test]
    fn every_block_and_every_list_item_of_the_fixtures_has_the_hash_of_its_own() {
        let mut items = 0;
        for (path, document) in fixture_documents() {
            let hashes = document.hashes();
            for (at, block) in document.blocks() {
                let name = format!("{} at {at:?}", path.display());
                assert_eq!(hashes.block(&at), Some(hash_value(block)), "{name}");
                if let Block::List(list) = block {
                    for (index, item) in list.items.iter().enumerate() {
                        assert_eq!(hashes.item(&at, index), Some(hash_value(item)), "{name}");
                        items += 1;
                    }
                }
            }
        }
        assert!(items > 20, "{items}");
    }

    #[test]
    fn the_same_block_has_the_same_hash_anywhere_and_wherever_it_was_written() {
        let mut placed = Document::new(vec![paragraph("x"), paragraph("a")]);
        placed.places.insert(vec![1], Place::file(99));
        let elsewhere = Document::new(vec![paragraph("a")]);
        assert_eq!(placed.hashes().block(&[1]), elsewhere.hashes().block(&[0]),);
        assert_ne!(placed.hashes().block(&[0]), placed.hashes().block(&[1]));
    }

    #[test]
    fn a_change_inside_a_block_changes_it_and_what_holds_it_and_nothing_else() {
        let build = |inner: &str| {
            Document::new(vec![
                paragraph("before"),
                Block::BlockQuote(vec![
                    paragraph("sibling"),
                    list(vec![
                        item(vec![paragraph(inner)]),
                        item(vec![paragraph("o")]),
                    ]),
                ]),
                paragraph("after"),
            ])
        };
        let (old, new) = (build("one").hashes(), build("two").hashes());
        let changed = |path: &[usize]| old.block(path) != new.block(path);
        // The paragraph, its list, the quote around them.
        assert!(changed(&[1, 1, 0, 0]) && changed(&[1, 1]) && changed(&[1]));
        // Not the sibling, the other item, or the blocks around the quote.
        assert!(!changed(&[1, 0]) && !changed(&[0]) && !changed(&[2]));
        assert!(!changed(&[1, 1, 1, 0]));
        assert_ne!(old.item(&[1, 1], 0), new.item(&[1, 1], 0));
        assert_eq!(old.item(&[1, 1], 1), new.item(&[1, 1], 1));
    }

    #[test]
    fn block_answers_for_blocks_only_and_item_for_items_only() {
        let document = Document::new(vec![list(vec![item(vec![paragraph("a")])])]);
        let hashes = document.hashes();
        // `[0, 0]` is the first item of the list, not a block.
        assert_eq!(hashes.block(&[0, 0]), None);
        assert!(hashes.block(&[0, 0, 0]).is_some());
        assert!(hashes.item(&[0], 0).is_some());
        // `[0]` is a block, and not an item of anything.
        assert_eq!(hashes.item(&[], 0), None);
        assert_eq!(hashes.item(&[0, 0, 0], 0), None);
        assert_eq!(hashes.item(&[0], 1), None);
    }

    fn item(blocks: Vec<Block>) -> ListItem {
        ListItem {
            checked: None,
            blocks,
        }
    }

    fn list(items: Vec<ListItem>) -> Block {
        Block::List(List {
            start: None,
            tight: true,
            items,
        })
    }

    #[test]
    fn a_leaf_block_has_no_contents_and_no_children() {
        let block = paragraph("a");
        assert!(matches!(block.contents(), Contents::None));
        assert_eq!(block.children().count(), 0);
    }

    #[test]
    fn a_quote_gives_one_step_to_each_block() {
        let quote = Block::BlockQuote(vec![paragraph("a"), paragraph("b")]);
        assert!(matches!(quote.contents(), Contents::Blocks(blocks) if blocks.len() == 2));
        let steps: Vec<Steps> = quote.children().map(|(steps, _)| steps).collect();
        assert_eq!(steps, [Steps::One(0), Steps::One(1)]);
    }

    #[test]
    fn a_list_gives_two_steps_to_each_block_of_each_item() {
        let list = list(vec![
            item(vec![paragraph("a"), paragraph("b")]),
            item(vec![]),
            item(vec![paragraph("c")]),
        ]);
        assert!(matches!(list.contents(), Contents::Items(items) if items.len() == 3));
        let steps: Vec<Steps> = list.children().map(|(steps, _)| steps).collect();
        assert_eq!(
            steps,
            [Steps::Two(0, 0), Steps::Two(0, 1), Steps::Two(2, 0)]
        );
    }

    #[test]
    fn steps_follow_a_path() {
        assert_eq!(Steps::One(3).after(&[1, 2]), [1, 2, 3]);
        assert_eq!(Steps::Two(3, 4).after(&[1]), [1, 3, 4]);
    }

    #[test]
    fn the_blocks_of_a_document_come_with_their_paths_each_before_those_inside_it() {
        let document = Document::new(vec![
            paragraph("a"),
            Block::Details {
                summary: vec![],
                blocks: vec![list(vec![item(vec![Block::BlockQuote(vec![paragraph(
                    "b",
                )])])])],
            },
            paragraph("c"),
        ]);
        let paths: Vec<Vec<usize>> = document
            .blocks()
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert_eq!(
            paths,
            [
                vec![0],
                vec![1],
                vec![1, 0],
                vec![1, 0, 0, 0],
                vec![1, 0, 0, 0, 0],
                vec![2]
            ]
        );
    }

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
