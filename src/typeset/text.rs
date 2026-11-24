//! What a document says, as text: the texts of each block, and the text of
//! the shown documents as a whole.
//!
//! What a document holds is asked of the model, and where it is shown is
//! asked of a layout. A search finds a term here, and a copy asks here for
//! the text between two places; neither looks at a line or a page.
//!
//! A block has a list of texts: the one text of a paragraph or a heading,
//! the code of a code block, a cell each of a table, the keys and values of
//! front matter, the summary of a `<details>`, the title of an alert, the
//! source of an HTML block. Kiri's own notes and errors are not the
//! document's text, and neither are the language of a code block, the
//! marker of a list item or anything else that a layout adds.
//!
//! Which texts a block has, and their numbers, are decided here and nowhere
//! else: a layout asks [`number`] for the text it is laying out, and takes
//! the number of a table's cell or a front matter's leaf from the grid
//! ([`grid`]), which numbers them in the order this module lists them.
//!
//! Where an inline goes in the text is one walk, [`flatten`], which also
//! tells the bytes of each inline in it, so that nothing else walks inlines
//! to find a link or an image.
//!
//! The texts of each block are made by `pieces` and `flatten`. Their numbers
//! are made by `number`, with `Part`, and `front_matter_texts`, which says
//! which texts front matter has, none for a blank source: both layouts ask
//! these, and none counts a text by itself. `DocumentText` is the text of the
//! shown documents: the texts by path, every place, `find_each` and
//! `text(range)`. `places` is the character comparison of a search.

use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

use crate::typeset::grid::{self, Cell, Grid, Leaves};
use crate::typeset::model::{Block, Document, FrontMatter, Image, Inline, Value};
use crate::typeset::position::{Pos, Within};
use crate::typeset::source_rows::SourceRows;

/// One text of a block, and what comes before it when the texts of a block
/// are read as one: a line break or a tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub before: Before,
    pub text: Text,
}

/// The text of a [`Piece`]: its own, or the start of a text that a block
/// holds already, as a code block's code, so that a large file is not kept
/// twice.
#[derive(Debug, Clone)]
pub enum Text {
    Own(String),
    /// The first `end` bytes of the text.
    Shared(Arc<str>, usize),
}

impl std::ops::Deref for Text {
    type Target = str;

    fn deref(&self) -> &str {
        match self {
            Self::Own(text) => text,
            Self::Shared(whole, end) => &whole[..*end],
        }
    }
}

impl std::fmt::Display for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self)
    }
}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for Text {}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Self::Own(text)
    }
}

/// What comes between a text and the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Before {
    /// A line break. The first text of a block has it, too: between blocks
    /// and files there is always a line break.
    Line,
    /// A tab: the next cell of a row of a table.
    Cell,
}

impl Piece {
    fn line(text: impl Into<String>) -> Self {
        Self {
            before: Before::Line,
            text: Text::Own(text.into()),
        }
    }

    /// A line of `whole` without the line break that ends it, which the
    /// piece shares and does not copy.
    fn shared(whole: &Arc<str>) -> Self {
        let end = without_last_break(whole).len();
        Self {
            before: Before::Line,
            text: Text::Shared(Arc::clone(whole), end),
        }
    }
}

/// Writes the text of `inlines` to `out`, and calls `visit` with each inline
/// and the bytes of its text in `out`, after the inline is written, so that
/// an inline inside another is visited before it.
///
/// Text, code and inline HTML are as they are; emphasis and the like, a
/// link, give the text of what is in them; an image gives its alt text; a
/// formula its LaTeX; a soft break a space, a hard break a line break.
/// (`Inline::plain_text` is another thing: it gives a formula as written
/// without math, and a hard break as a space.)
pub fn flatten(
    inlines: &[Inline],
    out: &mut String,
    visit: &mut impl FnMut(&Inline, Range<usize>),
) {
    for inline in inlines {
        let start = out.len();
        match inline {
            Inline::Text(text) | Inline::Code(text) | Inline::Html(text) => out.push_str(text),
            Inline::Emphasis(content)
            | Inline::Strong(content)
            | Inline::Strikethrough(content)
            | Inline::Kbd(content)
            | Inline::Mark(content)
            | Inline::Subscript(content)
            | Inline::Superscript(content)
            | Inline::Underline(content)
            | Inline::Link { content, .. }
            | Inline::Image(Image { alt: content, .. }) => flatten(content, out, visit),
            Inline::Formula { source, .. } => out.push_str(source),
            Inline::SoftBreak => out.push(' '),
            Inline::LineBreak => out.push('\n'),
            Inline::Anchor(_) => {}
        }
        visit(inline, start..out.len());
    }
}

/// How many bytes `inline` gives to the text of its block, as [`flatten`]
/// writes it. A layout that leaves an inline out, or shows something else
/// in its place, still moves on by this much.
pub fn inline_len(inline: &Inline) -> usize {
    match inline {
        Inline::Text(text) | Inline::Code(text) | Inline::Html(text) => text.len(),
        Inline::Emphasis(content)
        | Inline::Strong(content)
        | Inline::Strikethrough(content)
        | Inline::Kbd(content)
        | Inline::Mark(content)
        | Inline::Subscript(content)
        | Inline::Superscript(content)
        | Inline::Underline(content)
        | Inline::Link { content, .. }
        | Inline::Image(Image { alt: content, .. }) => content.iter().map(inline_len).sum(),
        Inline::Formula { source, .. } => source.len(),
        Inline::SoftBreak | Inline::LineBreak => 1,
        Inline::Anchor(_) => 0,
    }
}

/// Where a layout is in one text of a block as it lays out the inlines of
/// the text: which text, and how many of its bytes the inlines so far gave.
/// What it shows of the text, it takes; what it does not, it skips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    text: Option<u32>,
    at: u32,
}

impl Cursor {
    /// At the start of text number `text` of the block being laid out.
    pub fn new(text: usize) -> Self {
        Self {
            text: Some(u32::try_from(text).unwrap_or(u32::MAX)),
            at: 0,
        }
    }

    /// At no text: what is laid out has none, such as a note of kiri's.
    pub fn off() -> Self {
        Self { text: None, at: 0 }
    }

    /// The number of the text, if there is one.
    pub fn text(&self) -> Option<u32> {
        self.text
    }

    /// Takes the next `len` bytes and gives their range in the text.
    pub fn take(&mut self, len: usize) -> Range<u32> {
        let start = self.at;
        self.at = start.saturating_add(u32::try_from(len).unwrap_or(u32::MAX));
        start..self.at
    }

    /// Skips `inline`, which is not shown as text, and gives its range.
    pub fn skip(&mut self, inline: &Inline) -> Range<u32> {
        self.take(inline_len(inline))
    }
}

/// Which part of a block a text of it is, for the blocks that have one text
/// of each kind. The cells of a table and the leaves of front matter are
/// numbered by their grid, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The inlines of a heading or a paragraph.
    Content,
    /// The summary of a `<details>`.
    Summary,
    /// The source of a code block, an HTML block, or front matter shown as
    /// its source.
    Source,
    /// The title the author wrote on an alert.
    Title,
}

/// The number of the text of `block` that is `part`, or none when the block
/// has no such text.
pub fn number(block: &Block, part: Part) -> Option<usize> {
    let has = match block {
        Block::Heading { .. } | Block::Paragraph(_) => part == Part::Content,
        Block::Details { .. } => part == Part::Summary,
        Block::CodeBlock { .. } | Block::Html(_) => part == Part::Source,
        Block::Alert { title, .. } => part == Part::Title && title.is_some(),
        Block::FrontMatter(front_matter) => {
            part == Part::Source
                && matches!(front_matter_texts(front_matter), FrontMatterTexts::Source)
        }
        // A table's texts are its cells, numbered by its grid.
        Block::Table(_)
        | Block::BlockQuote(_)
        | Block::List(_)
        | Block::ThematicBreak
        | Block::Note(_)
        | Block::Error(_)
        | Block::ImageFile(_) => false,
    };
    has.then_some(0)
}

/// What the texts of front matter are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontMatterTexts<'a> {
    /// None: the source is empty or only white space.
    None,
    /// Its keys and values, as the leaves of its grid.
    Leaves(&'a [(String, Value)]),
    /// Its source: another language, content that does not parse, or
    /// content that is not a map.
    Source,
}

/// What the texts of `front_matter` are, however it is shown: shown as code
/// it shows the source in place of its leaves, but those are still what its
/// texts are.
pub fn front_matter_texts(front_matter: &FrontMatter) -> FrontMatterTexts<'_> {
    if front_matter.source.trim().is_empty() {
        return FrontMatterTexts::None;
    }
    match &front_matter.value {
        Some(Ok(Value::Map(entries))) => FrontMatterTexts::Leaves(entries),
        _ => FrontMatterTexts::Source,
    }
}

/// The texts of `block`. `visit` is called with the number of the text, an
/// inline in it and the bytes of the inline there, as [`flatten`] does.
pub fn pieces_with(
    block: &Block,
    visit: &mut impl FnMut(usize, &Inline, Range<usize>),
) -> Vec<Piece> {
    let mut pieces = Vec::new();
    // The one text a block has of `part` is where [`number`] says it is.
    let own = |pieces: &[Piece], part| {
        debug_assert_eq!(number(block, part), Some(pieces.len()));
    };
    let mut inlines = |pieces: &mut Vec<Piece>, before, content: &[Inline]| {
        let number = pieces.len();
        let mut text = String::new();
        flatten(content, &mut text, &mut |inline, bytes| {
            visit(number, inline, bytes);
        });
        pieces.push(Piece {
            before,
            text: text.into(),
        });
    };
    match block {
        Block::Heading { content, .. } | Block::Paragraph(content) => {
            own(&pieces, Part::Content);
            inlines(&mut pieces, Before::Line, content);
        }
        Block::Details { summary, .. } => {
            own(&pieces, Part::Summary);
            inlines(&mut pieces, Before::Line, summary);
        }
        Block::Table(table) => {
            // The cells are numbered by the grid, a row at a time.
            let numbers = grid::table_grid(table, |number, _, _| number);
            let (header, rows) = (table.header(), table.rows(0..table.row_count()));
            let rows = std::iter::once(&*header).chain(rows.iter().map(Vec::as_slice));
            for (row, numbers) in rows.zip(&numbers.rows) {
                for (column, (cell, number)) in row.iter().zip(numbers).enumerate() {
                    let Cell::Leaf(number) = number else {
                        unreachable!("a table's cells are leaves")
                    };
                    debug_assert_eq!(*number, pieces.len());
                    inlines(&mut pieces, before_cell(column), cell);
                }
            }
        }
        // The line break that ends the code or the HTML is not part of it:
        // the break between blocks is.
        Block::CodeBlock { code, .. } => {
            own(&pieces, Part::Source);
            pieces.push(Piece::shared(code));
        }
        Block::Html(source) => {
            own(&pieces, Part::Source);
            pieces.push(Piece::line(without_last_break(source)));
        }
        Block::Alert {
            title: Some(title), ..
        } => {
            own(&pieces, Part::Title);
            pieces.push(Piece::line(title.as_str()));
        }
        Block::FrontMatter(front_matter) => match front_matter_texts(front_matter) {
            FrontMatterTexts::None => {}
            FrontMatterTexts::Leaves(entries) => {
                leaves_of(&grid::front_matter_grid(entries, &LeafTexts), &mut pieces);
            }
            FrontMatterTexts::Source => {
                own(&pieces, Part::Source);
                pieces.push(Piece::line(without_last_break(&front_matter.source)));
            }
        },
        Block::Alert { title: None, .. }
        | Block::BlockQuote(_)
        | Block::List(_)
        | Block::ThematicBreak
        | Block::Note(_)
        | Block::Error(_)
        | Block::ImageFile(_) => {}
    }
    pieces
}

/// The texts of `block`.
pub fn pieces(block: &Block) -> Vec<Piece> {
    pieces_with(block, &mut |_, _, _| {})
}

/// Why a layout may leave a byte of a text unshown, for the tests of the
/// maps: what a layout is meant to leave out of a line of text.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gap {
    /// A space or a line break at a place where a line wraps, or at an edge
    /// of the text: the lines on each side of it are not the same.
    Wrap,
    /// A space that comes right after another, which a style that collapses
    /// spaces shows as one.
    Collapsed,
    /// The carriage return of a line ending, which no layout shows.
    CarriageReturn,
}

/// How a byte of a text is shown, when a line shows it.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shown {
    /// The number of the line that shows it.
    pub line: usize,
    /// Whether the style of the text it is in collapses runs of spaces.
    pub collapses: bool,
}

/// Whether the byte `at` of `text`, which no line shows, is one that a layout
/// leaves out, and why. `shown` says for each byte of `text` how a line
/// shows it, if one does.
#[cfg(test)]
pub(crate) fn excused_gap(text: &str, shown: &[Option<Shown>], at: usize) -> Option<Gap> {
    let c = text[at..].chars().next()?;
    match c {
        '\r' if text[at + 1..].starts_with('\n') => Some(Gap::CarriageReturn),
        ' ' | '\n' => {
            let before = (0..at).rev().find_map(|byte| shown[byte]);
            // Only a style that collapses spaces drops the second of two.
            if c == ' ' && text[..at].ends_with(' ') && before.is_some_and(|b| b.collapses) {
                return Some(Gap::Collapsed);
            }
            let after = (at..text.len()).find_map(|byte| shown[byte]);
            match (before, after) {
                (Some(before), Some(after)) if before.line == after.line => None,
                _ => Some(Gap::Wrap),
            }
        }
        _ => None,
    }
}

/// `text` without the line ending that ends it, `\n` or `\r\n`.
pub(crate) fn without_last_break(text: &str) -> &str {
    text.strip_suffix('\n')
        .map_or(text, |text| text.strip_suffix('\r').unwrap_or(text))
}

/// Front matter's leaves as texts, as the table of its keys and values has
/// them, each with the number its grid gave it.
struct LeafTexts;

type Numbered = (usize, String);

impl Leaves<Numbered> for LeafTexts {
    fn key(&self, number: usize, key: &str) -> Numbered {
        (number, key.to_owned())
    }

    fn text(&self, number: usize, text: &str) -> Numbered {
        (number, text.to_owned())
    }

    fn empty(&self, number: usize) -> Numbered {
        (number, String::new())
    }

    fn yaml(&self, number: usize, yaml: &str) -> Numbered {
        (number, without_last_break(yaml).to_owned())
    }
}

/// The leaves of `grid` as texts, a row at a time: the first leaf of a row
/// after a line break, the others after a tab. A grid in a cell starts its
/// rows on lines of their own, and so does each of the cells one below
/// another.
fn leaves_of(grid: &Grid<Numbered>, pieces: &mut Vec<Piece>) {
    for row in &grid.rows {
        for (column, cell) in row.iter().enumerate() {
            let before = if column == 0 {
                Before::Line
            } else {
                Before::Cell
            };
            cell_leaves(cell, before, pieces);
        }
    }
}

fn cell_leaves(cell: &Cell<Numbered>, before: Before, pieces: &mut Vec<Piece>) {
    match cell {
        Cell::Leaf((number, text)) => {
            // The grid numbers its leaves in the order they are listed here.
            debug_assert_eq!(*number, pieces.len());
            pieces.push(Piece {
                before,
                text: text.clone().into(),
            });
        }
        Cell::Table(grid) => leaves_of(grid, pieces),
        Cell::Stack(cells) => {
            for (index, cell) in cells.iter().enumerate() {
                let before = if index == 0 { before } else { Before::Line };
                cell_leaves(cell, before, pieces);
            }
        }
    }
}

/// The bytes of each place of `term` in `text`, from left to right and
/// none overlapping another.
pub fn places(text: &str, term: &[char], ignore_case: bool) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    if term.is_empty() {
        return found;
    }
    // Where the next place may start. The text is walked as it is, with no
    // list of its characters, which for a large file would take many
    // times its size.
    let mut rest = text.char_indices();
    loop {
        let mut candidate = rest.clone();
        let matches = (term.iter())
            .all(|&t| (candidate.next()).is_some_and(|(_, c)| same(c, t, ignore_case)));
        if matches {
            found.push(rest.offset()..candidate.offset());
            rest = candidate;
        } else if rest.next().is_none() {
            return found;
        }
    }
}

/// Whether the characters `a` and `b` match: a no-break space matches a
/// space, and with `ignore_case` an upper-case letter its lower case.
fn same(a: char, b: char, ignore_case: bool) -> bool {
    let plain = |c| if c == '\u{a0}' { ' ' } else { c };
    let (a, b) = (plain(a), plain(b));
    a == b || (ignore_case && a.to_lowercase().eq(b.to_lowercase()))
}

/// What comes before the cell of a table in `column`: a line break before
/// the first of a row, a tab before the others.
fn before_cell(column: usize) -> Before {
    match column {
        0 => Before::Line,
        _ => Before::Cell,
    }
}

/// The texts of a block as the text of the documents holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Held {
    /// Each text, made with the text of the documents.
    Pieces(Vec<Piece>),
    /// The rows of a table of a CSV's source: a text a cell, numbered a row
    /// at a time from the header's first, as the table's grid numbers them,
    /// and read from the source when it is asked for.
    Rows(SourceRows),
}

/// The texts of one block, by number. A table of a CSV's source keeps none:
/// it reads each from the source when it is asked for, so one text costs
/// the reading of its row, and texts in order the reading of their rows.
#[derive(Debug, Clone, Copy)]
pub struct Texts<'a>(Of<'a>);

#[derive(Debug, Clone, Copy)]
enum Of<'a> {
    Pieces(&'a [Piece]),
    Rows(&'a SourceRows),
}

impl<'a> Texts<'a> {
    fn of(held: &'a Held) -> Self {
        Self(match held {
            Held::Pieces(pieces) => Of::Pieces(pieces),
            Held::Rows(rows) => Of::Rows(rows),
        })
    }

    /// How many texts there are.
    pub fn len(&self) -> usize {
        match self.0 {
            Of::Pieces(pieces) => pieces.len(),
            Of::Rows(rows) => rows.len() * rows.columns(),
        }
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Text number `number`, if there is one.
    pub fn get(&self, number: usize) -> Option<Cow<'a, Piece>> {
        if let Of::Pieces(pieces) = self.0 {
            return pieces.get(number).map(Cow::Borrowed);
        }
        let mut found = None;
        self.each(number..number + 1, |_, before, text| {
            found = Some(Cow::Owned(Piece {
                before,
                text: text.to_owned().into(),
            }));
        });
        found
    }

    /// The last text, if there is one.
    pub fn last(&self) -> Option<Cow<'a, Piece>> {
        self.get(self.len().checked_sub(1)?)
    }

    /// Calls `each` with the number of each text of `numbers` there is, in
    /// order, with what comes before it and the text. A table of a source
    /// reads the rows of those texts once, and keeps none of them.
    pub fn each(&self, numbers: Range<usize>, mut each: impl FnMut(usize, Before, &str)) {
        let numbers = numbers.start..numbers.end.min(self.len());
        if numbers.is_empty() {
            return;
        }
        match self.0 {
            Of::Pieces(pieces) => {
                for (number, piece) in pieces[numbers.clone()].iter().enumerate() {
                    each(numbers.start + number, piece.before, &piece.text);
                }
            }
            Of::Rows(rows) => {
                let columns = rows.columns();
                let row_numbers = numbers.start / columns..(numbers.end - 1) / columns + 1;
                rows.texts(row_numbers, |row, column, text| {
                    let number = row * columns + column;
                    if numbers.contains(&number) {
                        each(number, before_cell(column), text);
                    }
                });
            }
        }
    }

    /// Every text, made.
    #[cfg(test)]
    pub(crate) fn to_vec(self) -> Vec<Piece> {
        let mut out = Vec::new();
        self.each(0..self.len(), |_, before, text| {
            out.push(Piece {
                before,
                text: text.to_owned().into(),
            });
        });
        out
    }
}

/// The texts of the blocks of the shown documents, by file and path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentText {
    /// By file, each block with its path, in the order of the paths.
    files: Vec<Vec<(Vec<usize>, Held)>>,
}

impl DocumentText {
    /// The text of `files`, shown in this order: file 0 is the first.
    pub fn new<'a>(files: impl IntoIterator<Item = &'a Document>) -> Self {
        let held = |block: &Block| match block {
            Block::Table(table) if let Some(rows) = table.source() => Held::Rows(rows.clone()),
            block => Held::Pieces(pieces(block)),
        };
        Self {
            files: (files.into_iter())
                .map(|document| {
                    (document.blocks().into_iter())
                        .map(|(path, block)| (path, held(block)))
                        .collect()
                })
                .collect(),
        }
    }

    /// The texts of the block at `path` of file `file`; none for a block
    /// that has none, or one that is not there.
    pub fn texts(&self, file: usize, path: &[usize]) -> Texts<'_> {
        let none = Texts(Of::Pieces(&[]));
        let Some(blocks) = self.files.get(file) else {
            return none;
        };
        // The blocks are in the order of the paths, which is the order of
        // `Vec`s compared as lists.
        match blocks.binary_search_by(|(at, _)| at.as_slice().cmp(path)) {
            Ok(index) => Texts::of(&blocks[index].1),
            Err(_) => none,
        }
    }

    /// The places of `term` that [`find_each`](Self::find_each) gives, as
    /// ranges.
    #[cfg(test)]
    pub(crate) fn find(&self, term: &str, ignore_case: bool) -> Vec<Range<Pos>> {
        let mut found = Vec::new();
        self.find_each(term, ignore_case, |file, path, number, bytes| {
            found.push(
                Pos::text(file, path, number, bytes.start)
                    ..Pos::text(file, path, number, bytes.end),
            );
        });
        found
    }

    /// Calls `each` with the places of `term` in the texts, from the start
    /// and from left to right, none overlapping another, each as the file,
    /// the path of the block, the number of the text and its bytes, so that
    /// the caller keeps them as it will. A term is found in one text: it is
    /// not found across the cells of a table or across blocks, and is found
    /// across a soft break, which is a space. A no-break space matches a
    /// space; with `ignore_case`, a letter matches its other case. The
    /// bytes of a place are those of the original text.
    pub fn find_each(
        &self,
        term: &str,
        ignore_case: bool,
        mut each: impl FnMut(usize, &[usize], usize, Range<usize>),
    ) {
        let term: Vec<char> = term.chars().collect();
        for (file, blocks) in self.files.iter().enumerate() {
            for (path, held) in blocks {
                let texts = Texts::of(held);
                texts.each(0..texts.len(), |number, _, text| {
                    for bytes in places(text, &term, ignore_case) {
                        each(file, path, number, bytes);
                    }
                });
            }
        }
    }

    /// The text between the two ends of `range`, as a copy would give it:
    /// a line break between texts that are on lines of their own, and between
    /// blocks and files, and a tab between the cells of a row. A range that
    /// is empty or backwards gives nothing. A place that is in the middle of
    /// a character, or past the end of a text, is moved to the nearest
    /// place there is.
    pub fn text(&self, range: &Range<Pos>) -> String {
        let mut out = String::new();
        let mut started = false;
        if range.start >= range.end {
            return out;
        }
        for (file, blocks) in self.files.iter().enumerate().skip(range.start.file) {
            if file > range.end.file {
                break;
            }
            for (path, held) in blocks {
                let texts = Texts::of(held);
                // The texts that may be in the range: none before the first
                // that may end after its start, and none from the first that
                // starts at its end.
                let first_after = |number| Pos::text(file, path, number, usize::MAX) > range.start;
                let at_end = |number| Pos::text(file, path, number, 0) >= range.end;
                let numbers =
                    first_where(texts.len(), first_after)..first_where(texts.len(), at_end);
                texts.each(numbers, |number, before, text| {
                    let first = Pos::text(file, path, number, 0);
                    let last = Pos::text(file, path, number, text.len());
                    // A text is in the range when some of it is, or when it
                    // is empty and starts in it.
                    let some_of_it = match text.is_empty() {
                        true => first >= range.start && first < range.end,
                        false => last > range.start && first < range.end,
                    };
                    if !some_of_it {
                        return;
                    }
                    let from = Self::offset(&range.start, &first, text).unwrap_or(0);
                    let to = Self::offset(&range.end, &first, text).unwrap_or(text.len());
                    if started {
                        out.push(match before {
                            Before::Line => '\n',
                            Before::Cell => '\t',
                        });
                    }
                    started = true;
                    out.push_str(&text[from..to.max(from)]);
                });
            }
        }
        out
    }

    /// The byte of `text` that `at` names, moved back to a character, if
    /// `at` is a place in the text whose first place is `first`.
    fn offset(at: &Pos, first: &Pos, text: &str) -> Option<usize> {
        match (&at.within, &first.within) {
            (
                Within::Text {
                    text: number,
                    offset,
                },
                Within::Text { text: mine, .. },
            ) if (at.file, &at.path, number) == (first.file, &first.path, mine) => {
                let mut offset = (*offset).min(text.len());
                while !text.is_char_boundary(offset) {
                    offset -= 1;
                }
                Some(offset)
            }
            _ => None,
        }
    }

    /// The paths of the blocks of file `file`, each before the blocks inside
    /// it, as the blocks of its document come; none for a file that is not
    /// there.
    pub fn paths(&self, file: usize) -> impl Iterator<Item = &[usize]> {
        (self.files.get(file).into_iter().flatten()).map(|(path, _)| path.as_slice())
    }

    /// Every place there is to name in the shown documents, in the order of
    /// the documents: for each block, itself and the start of each of its
    /// texts, and its end after the blocks inside it.
    pub fn positions(&self) -> impl Iterator<Item = Pos> + '_ {
        self.files.iter().enumerate().flat_map(|(file, blocks)| {
            let mut open: Vec<&[usize]> = Vec::new();
            let mut out = Vec::new();
            for (path, held) in blocks {
                let count = Texts::of(held).len();
                while let Some(last) = open.last().filter(|last| !path.starts_with(last)) {
                    out.push(Pos::end(file, last));
                    open.pop();
                }
                out.push(Pos::block(file, path));
                out.extend((0..count).map(|text| Pos {
                    file,
                    path: path.clone(),
                    within: Within::Text { text, offset: 0 },
                }));
                open.push(path);
            }
            while let Some(last) = open.pop() {
                out.push(Pos::end(file, last));
            }
            out
        })
    }
}

/// The first of `0..len` for which `holds` is true, or `len`; `holds` is
/// false and then true, once, along `0..len`.
fn first_where(len: usize, holds: impl Fn(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, len);
    while low < high {
        let middle = low + (high - low) / 2;
        match holds(middle) {
            true => high = middle,
            false => low = middle + 1,
        }
    }
    low
}

#[cfg(test)]
mod props;

#[cfg(test)]
mod tests;
