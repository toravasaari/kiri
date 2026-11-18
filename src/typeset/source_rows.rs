//! The rows of a CSV or TSV file as a table holds them: the source, held
//! once, and where each row starts in it, made into cells or texts when
//! they are asked for. The CSV parser (`parse/csv.rs`) finds the starts
//! and builds one; the document model holds it in a table, and the
//! document's text reads the texts of its cells from it.

use std::ops::Range;
use std::sync::Arc;

use csv::{Reader, ReaderBuilder, StringRecord};
use unicode_width::UnicodeWidthStr;

use crate::typeset::model::Inline;

/// What separates the fields of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Separator {
    Comma,
    Tab,
}

/// A reader of the CSV in `bytes`, its fields split at `separator`.
pub(crate) fn reader(bytes: &[u8], separator: Separator) -> Reader<&[u8]> {
    #[cfg(test)]
    READERS_MADE.with(|made| made.set(made.get() + 1));
    let delimiter = match separator {
        Separator::Comma => b',',
        Separator::Tab => b'\t',
    };
    ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes)
}

/// The rows of a CSV or TSV file, the header's first: its source, held
/// once, and where each row starts in it. The cells of a row are made when
/// they are asked for, from the source, as the reading of the whole file
/// made them.
///
/// Two are equal when the sources of their rows and their separators are,
/// and the hash is of the same: neither reads a cell. The rows follow from
/// the bytes up to the end of the last row.
///
/// A table that is shown capped, as print mode and reading mode show a
/// large one, has a view of its first rows ([`SourceRows::capped`]).
#[derive(Clone)]
pub struct SourceRows {
    /// The source, up to the end of the last row at least.
    source: Arc<str>,
    separator: Separator,
    /// Where each row starts in `source`, and where the last ends. A row
    /// starts where the one before it ended, so the blank lines before it
    /// are part of it, and the reader skips them.
    starts: Arc<[usize]>,
    /// The fields of the widest row: every row has as many cells.
    columns: usize,
    /// The texts each row has in the document's text, which numbers the
    /// cells a row at a time: `columns`, but in a capped view, whose rows
    /// are numbered as the whole table's are.
    stride: usize,
    /// The rows a capped view keeps, the header's included, and the fields of
    /// the widest of them, when the table has more cells than a capped view
    /// shows.
    cap: Option<(usize, usize)>,
    /// What the texts of each column are as found when the rows were: for
    /// the widths of the columns of a long table in the pager.
    texts: Arc<ColumnTexts>,
}

/// What the fields of each column of a CSV are, as the parse finds them in
/// its one pass over the rows: the cells the widest takes and its longest
/// word, of the fields that are one line of text, and the rows with a field
/// that is not, which a layout measures as it does a cell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnTexts {
    /// The cells of the widest field of each column, and of its longest run
    /// without a space.
    pub widest: Vec<usize>,
    pub word: Vec<usize>,
    /// The rows, the header's 0, with a field that holds a tab or another
    /// control character, which a span shows otherwise, and the columns of
    /// such fields.
    pub other: Vec<usize>,
    pub other_columns: Vec<usize>,
}

impl ColumnTexts {
    /// Takes in `field`, of the column `column` of the row `row`.
    pub(crate) fn add(&mut self, row: usize, column: usize, field: &[u8]) {
        if column >= self.widest.len() {
            self.widest.resize(column + 1, 0);
            self.word.resize(column + 1, 0);
        }
        // A line break in a field is a space in its cell, as `texts` gives
        // it: `\r\n`, `\r` and `\n` one each.
        let spaced;
        let field = match field.iter().any(|&byte| byte == b'\r' || byte == b'\n') {
            true => {
                spaced = spaced_breaks(field);
                &spaced[..]
            }
            false => field,
        };
        let measured = match field.iter().all(|byte| (b' '..=b'~').contains(byte)) {
            // Printable ASCII is a cell a letter.
            true => Some((
                field.len(),
                (field.split(|&byte| byte == b' ')).map(<[u8]>::len).max(),
            )),
            false => std::str::from_utf8(field)
                .ok()
                .filter(|text| !text.chars().any(char::is_control))
                .map(|text| {
                    (
                        text.width(),
                        text.split(' ').map(UnicodeWidthStr::width).max(),
                    )
                }),
        };
        match measured {
            Some((widest, word)) => {
                self.widest[column] = self.widest[column].max(widest);
                self.word[column] = self.word[column].max(word.unwrap_or(0));
            }
            None => {
                if self.other.last() != Some(&row) {
                    self.other.push(row);
                }
                if !self.other_columns.contains(&column) {
                    self.other_columns.push(column);
                }
            }
        }
    }
}

/// What a parse has found of the rows of a source so far: where each row
/// starts and where the last ends, the fields of the widest row, the rows
/// and columns of a capped view once a row does not fit in it, and what
/// the fields of each column are. A parse of a source that grew goes on
/// from what it found of the rows before ([`SourceRows::carried`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub starts: Vec<usize>,
    pub columns: usize,
    pub cap: Option<(usize, usize)>,
    pub texts: ColumnTexts,
}

/// `field` with each line break, `\r\n`, `\r` or `\n`, a space.
fn spaced_breaks(field: &[u8]) -> Vec<u8> {
    let mut spaced = Vec::with_capacity(field.len());
    let mut bytes = field.iter().peekable();
    while let Some(&byte) = bytes.next() {
        match byte {
            b'\r' => {
                bytes.next_if_eq(&&b'\n');
                spaced.push(b' ');
            }
            b'\n' => spaced.push(b' '),
            byte => spaced.push(byte),
        }
    }
    spaced
}

#[cfg(test)]
thread_local! {
    /// How many cells were made of the rows of a source on this thread,
    /// which the tests of who asks for them count.
    static CELLS_MADE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// How many readers of a source were made on this thread.
    static READERS_MADE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many readers of a source were made on this thread, each of which
/// costs a buffer and some microseconds.
#[cfg(test)]
pub(crate) fn readers_made() -> usize {
    READERS_MADE.with(std::cell::Cell::get)
}

/// How many cells were made of the rows of a source on this thread.
#[cfg(test)]
pub(crate) fn cells_made() -> usize {
    CELLS_MADE.with(std::cell::Cell::get)
}

impl SourceRows {
    /// The rows of `source`, split at `separator`: each starts where
    /// `starts` says and ends where the next starts, the last at the last
    /// start, which is where `source` ends; the widest has `columns` fields.
    /// A capped view keeps the rows and columns that `cap` says.
    pub(crate) fn new(
        source: Arc<str>,
        separator: Separator,
        starts: Arc<[usize]>,
        (columns, cap): (usize, Option<(usize, usize)>),
        texts: ColumnTexts,
    ) -> Self {
        debug_assert_eq!(starts.last(), Some(&source.len()));
        Self {
            source,
            separator,
            starts,
            columns,
            stride: columns,
            cap,
            texts: Arc::new(texts),
        }
    }

    /// The view of the first rows that a table shown capped shows, when it
    /// has more cells than that: those rows, each as wide as the widest of
    /// them. Its cells keep the numbers they have in the whole table.
    pub fn capped(&self) -> Option<Self> {
        let (rows, columns) = self.cap?;
        Some(Self {
            source: Arc::clone(&self.source),
            separator: self.separator,
            starts: Arc::from(&self.starts[..=rows]),
            columns,
            stride: self.stride,
            cap: None,
            texts: Arc::clone(&self.texts),
        })
    }

    /// What a parse of `source`, split at `separator`, goes on from when
    /// `source` starts with the source of these rows, as a parse made them
    /// (not a capped view), split so too: what the parse found of them.
    /// `None` for another separator or another start.
    pub(crate) fn carried(&self, source: &str, separator: Separator) -> Option<Found> {
        let starts = source.as_bytes().starts_with(self.bytes().as_bytes());
        if !starts || separator != self.separator {
            return None;
        }
        Some(self.found())
    }

    /// What the parse found of these rows.
    fn found(&self) -> Found {
        Found {
            starts: self.starts.to_vec(),
            columns: self.columns,
            cap: self.cap,
            texts: ColumnTexts::clone(&self.texts),
        }
    }

    /// Whether `new` is these rows grown at their end: split at the same
    /// separator, with the source of these rows at its start, byte for byte.
    pub(crate) fn grew_into(&self, new: &Self) -> bool {
        self.separator == new.separator
            && new.bytes().as_bytes().starts_with(self.bytes().as_bytes())
    }

    /// What the parse found of these rows, for the tests that compare two
    /// parses.
    #[cfg(test)]
    pub(crate) fn parts(&self) -> Found {
        self.found()
    }

    /// What the fields of each column are, as the parse found them.
    pub fn column_texts(&self) -> &ColumnTexts {
        &self.texts
    }

    /// How many rows the whole table has past those of its capped view.
    pub fn left_out(&self) -> usize {
        self.cap.map_or(0, |(rows, _)| self.len() - rows)
    }

    /// The number among the texts of the document of the first cell of the
    /// row `row`, the header's 0.
    pub fn first_text(&self, row: usize) -> usize {
        row * self.stride
    }

    /// The bytes of the source that the rows are, up to the end of the last.
    fn bytes(&self) -> &str {
        &self.source[..self.starts[self.starts.len() - 1]]
    }

    /// How many rows there are, the header's included.
    pub fn len(&self) -> usize {
        self.starts.len() - 1
    }

    /// Whether there is no row: never, for a table has its header.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many cells a row has.
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// `rows` cut at the last row.
    fn cut(&self, rows: Range<usize>) -> Range<usize> {
        let end = rows.end.min(self.len());
        rows.start.min(end)..end
    }

    /// The cells of the rows `rows`, made from the source, each row as
    /// wide as the widest. A range past the last row is cut at it.
    pub fn cells(&self, rows: Range<usize>) -> Vec<Vec<Vec<Inline>>> {
        let rows = self.cut(rows);
        #[cfg(test)]
        CELLS_MADE.with(|made| made.set(made.get() + rows.len() * self.columns));
        let mut made: Vec<Vec<Vec<Inline>>> = Vec::with_capacity(rows.len());
        self.texts(rows, |_, column, text| {
            if column == 0 {
                made.push(Vec::with_capacity(self.columns));
            }
            if let Some(row) = made.last_mut() {
                row.push(cell(text));
            }
        });
        made
    }

    /// Calls `each` with the row, the column and the text of each cell of
    /// the rows `rows`, a row at a time and from left to right, an empty
    /// one where a short row is padded. The text is the field with its line
    /// breaks turned into spaces, as its cell has it; no cell is made. A
    /// range past the last row is cut at it.
    pub fn texts(&self, rows: Range<usize>, mut each: impl FnMut(usize, usize, &str)) {
        let rows = self.cut(rows);
        if rows.is_empty() {
            return;
        }
        let (start, end) = (self.starts[rows.start], self.starts[rows.end]);
        // A reader takes a byte order mark at the start of what it reads as
        // the file's, and leaves it out; one at the start of a later row is
        // text. So a later row is read from the line break that ends the row
        // before it, which the reader skips as a blank line.
        let from = match start {
            0 => 0,
            start => {
                debug_assert!(matches!(self.source.as_bytes()[start - 1], b'\n' | b'\r'));
                start - 1
            }
        };
        let mut reader = reader(&self.source.as_bytes()[from..end], self.separator);
        let mut record = StringRecord::new();
        for row in rows {
            // The rows were found by reading the same bytes.
            let read = reader.read_record(&mut record).expect("a str is UTF-8");
            debug_assert!(read, "row {row} is in its bytes");
            for column in 0..self.columns {
                let field = record.get(column).unwrap_or_default();
                let spaced;
                let text = match field.contains(['\r', '\n']) {
                    true => {
                        spaced = field.replace("\r\n", " ").replace(['\r', '\n'], " ");
                        &spaced
                    }
                    false => field,
                };
                each(row, column, text);
            }
        }
    }
}

impl PartialEq for SourceRows {
    fn eq(&self, other: &Self) -> bool {
        self.separator == other.separator
            && ((Arc::ptr_eq(&self.source, &other.source)
                && self.starts.last() == other.starts.last())
                || self.bytes() == other.bytes())
    }
}

impl Eq for SourceRows {}

impl std::hash::Hash for SourceRows {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.separator.hash(state);
        self.bytes().hash(state);
    }
}

impl std::fmt::Debug for SourceRows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceRows")
            .field("separator", &self.separator)
            .field("rows", &self.len())
            .field("columns", &self.columns)
            .field("bytes", &self.source.len())
            .finish()
    }
}

/// The cell holding `text`, a field with its line breaks turned into
/// spaces.
pub(crate) fn cell(text: &str) -> Vec<Inline> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![Inline::Text(text.to_owned())]
}
