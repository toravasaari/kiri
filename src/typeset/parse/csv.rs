//! CSV and TSV source to document model: one table, the first row as its
//! header, as GitHub shows such a file.

use std::ops::Range;
use std::sync::Arc;

use csv::{ByteRecord, Position, Reader, ReaderBuilder, StringRecord};

use crate::typeset::model::{Alignment, Block, Document, Inline, Place, Table};

/// What separates the fields of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Separator {
    Comma,
    Tab,
}

/// The most cells a table shows, header included. Laying out a cell takes
/// about 900 bytes, so a larger file could run out of memory; rows past
/// the limit are left out with a note saying how many.
pub const MAX_CELLS: usize = 100_000;

/// A header row alone with more cells than `MAX_CELLS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooWide {
    pub columns: usize,
}

impl std::fmt::Display for TooWide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "too wide to show: {} columns, and kiri shows at most {} cells",
            grouped(self.columns, ' '),
            grouped(MAX_CELLS, ' ')
        )
    }
}

/// The table in `source`. Cells are plain text; quoted fields follow
/// RFC 4180, and a newline inside one becomes a space. Blank lines are
/// skipped, short rows are padded with empty cells, and malformed quotes
/// are read as leniently as they can be rather than refused.
///
/// The table keeps the source and where each row starts in it, and makes
/// the cells of a row only when they are asked for ([`SourceRows`]).
pub fn parse(source: &str, separator: Separator) -> Result<Document, TooWide> {
    parse_at_most(source, separator, MAX_CELLS)
}

/// The table in `source` with at most `max_cells` cells.
fn parse_at_most(
    source: &str,
    separator: Separator,
    max_cells: usize,
) -> Result<Document, TooWide> {
    let mut reader = reader(source.as_bytes(), separator);
    // Where each row kept starts, and where the last of them ends.
    let mut starts = vec![0];
    let mut columns = 0;
    // Rows past the limit are only counted, not kept.
    let mut left_out = 0;
    let mut record = ByteRecord::new();
    // Reading from a slice cannot fail, and records may differ in length.
    while reader
        .read_byte_record(&mut record)
        .expect("a slice can be read")
    {
        if left_out == 0 {
            let rows = starts.len() - 1;
            let wider = columns.max(record.len());
            if (rows + 1).saturating_mul(wider) <= max_cells {
                columns = wider;
                starts.push(byte(reader.position()));
                continue;
            }
            if rows == 0 {
                return Err(TooWide {
                    columns: record.len(),
                });
            }
        }
        left_out += 1;
    }
    if starts.len() == 1 {
        return Ok(Document::default());
    }
    // Rows past the limit are not read again: the table keeps the source
    // up to the end of the rows it has.
    let end = starts[starts.len() - 1];
    let rows = SourceRows {
        source: Arc::from(&source[..end]),
        separator,
        starts: starts.into(),
        columns,
    };
    let alignments = vec![Alignment::Left; columns];
    let mut blocks = vec![Block::Table(Table::from_source(alignments, rows))];
    if left_out > 0 {
        let rows = if left_out == 1 { "row" } else { "rows" };
        blocks.push(Block::Note(format!(
            "… {} more {rows} not shown",
            grouped(left_out, '\u{a0}')
        )));
    }
    let mut document = Document::new(blocks);
    document.places.insert(vec![0], Place::file(source.len()));
    Ok(document)
}

/// A reader of the CSV in `bytes`, its fields split at `separator`.
fn reader(bytes: &[u8], separator: Separator) -> Reader<&[u8]> {
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

/// The byte `position` is at.
fn byte(position: &Position) -> usize {
    usize::try_from(position.byte()).expect("a position in a str fits a usize")
}

/// The rows of a CSV or TSV file, the header's first: its source, held
/// once, and where each row starts in it. The cells of a row are made when
/// they are asked for, from the source, as the reading of the whole file
/// made them.
///
/// Two are equal when the sources of their rows, their separators and
/// their numbers of rows are, and the hash is of the same: neither reads a
/// cell.
#[derive(Clone)]
pub struct SourceRows {
    /// The source up to the end of the last row.
    source: Arc<str>,
    separator: Separator,
    /// Where each row starts in `source`, and where the last ends. A row
    /// starts where the one before it ended, so the blank lines before it
    /// are part of it, and the reader skips them.
    starts: Arc<[usize]>,
    /// The fields of the widest row: every row has as many cells.
    columns: usize,
}

#[cfg(test)]
thread_local! {
    /// How many cells were made of the rows of a source on this thread,
    /// which the tests of who asks for them count.
    static CELLS_MADE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many cells were made of the rows of a source on this thread.
#[cfg(test)]
pub(crate) fn cells_made() -> usize {
    CELLS_MADE.with(std::cell::Cell::get)
}

impl SourceRows {
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

    /// The cells of the rows `rows`, made from the source, each row as
    /// wide as the widest.
    pub fn cells(&self, rows: Range<usize>) -> Vec<Vec<Vec<Inline>>> {
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
    /// breaks turned into spaces, as its cell has it; no cell is made.
    pub fn texts(&self, rows: Range<usize>, mut each: impl FnMut(usize, usize, &str)) {
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
        (self.separator, self.len()) == (other.separator, other.len())
            && (Arc::ptr_eq(&self.source, &other.source) || self.source == other.source)
    }
}

impl Eq for SourceRows {}

impl std::hash::Hash for SourceRows {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.separator, self.len()).hash(state);
        self.source.hash(state);
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

/// `n` with its digits in groups of three, joined by `separator`.
fn grouped(n: usize, separator: char) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(digit);
    }
    out
}

/// The cell holding `text`, a field with its line breaks turned into
/// spaces.
fn cell(text: &str) -> Vec<Inline> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![Inline::Text(text.to_owned())]
}

#[cfg(test)]
mod props;
#[cfg(test)]
mod tests;
