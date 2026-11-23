//! CSV and TSV source to document model: one table, the first row as its
//! header, as GitHub shows such a file. The table holds the rows of the
//! source ([`SourceRows`]), which this finds the starts of.
//!
//! The parser finds the starts of the rows and, in the same pass, the column
//! texts (`ColumnTexts`), and builds a `SourceRows`, so that the model and the
//! text use no parser module. That the parser also measures the widths of texts
//! is a departure from keeping measurement out of parsing.

use std::sync::Arc;

use csv::{ByteRecord, Position};

use crate::typeset::model::{Alignment, Block, Document, Place, Table};
pub use crate::typeset::source_rows::Separator;
use crate::typeset::source_rows::{ColumnTexts, Found, SourceRows, reader};

/// The most cells a table shows, header included, where it is laid out
/// whole: in print mode and in reading mode. Laying out a cell takes about
/// 900 bytes, so a larger file could run out of memory; rows past the limit
/// are left out with a note saying how many. The pager's cell layout lays a
/// long table out in stretches and shows every row.
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

/// The table in `source`, which a capped view shows with at most
/// `max_cells` cells, and a note of the rows it leaves out after it.
fn parse_at_most(
    source: &str,
    separator: Separator,
    max_cells: usize,
) -> Result<Document, TooWide> {
    let found = Found {
        starts: vec![0],
        columns: 0,
        cap: None,
        texts: ColumnTexts::default(),
    };
    let found = read_on(source, separator, max_cells, found)?;
    Ok(document(source, separator, found))
}

/// The table in `source`, which `old`, the table of a CSV parsed before
/// with `separator`, may have grown into: when `source` starts with the
/// source of `old`'s rows, and its last row reads in `source` as it read
/// in the old one, the rows are taken as they are and the parse goes on
/// after them. What it finds is what a parse of the whole of `source`
/// finds. `None` when `old` is not such a table, or `source` does not
/// start so, and the whole is to be parsed.
pub fn parse_grown(
    old: &Document,
    source: &str,
    separator: Separator,
) -> Option<Result<Document, TooWide>> {
    parse_grown_at_most(old, source, separator, MAX_CELLS)
}

/// [`parse_grown`], for a capped view of at most `max_cells` cells.
fn parse_grown_at_most(
    old: &Document,
    source: &str,
    separator: Separator,
    max_cells: usize,
) -> Option<Result<Document, TooWide>> {
    let [Block::Table(table), ..] = &old.blocks[..] else {
        return None;
    };
    let found = table.source()?.carried(source, separator)?;
    let starts = &found.starts;
    // A last row that what came after it made longer, as a field it ended
    // in or a line break of two bytes it ended with, has other fields: the
    // whole is parsed.
    let (last, end) = (starts[starts.len() - 2], starts[starts.len() - 1]);
    // A reader takes a byte order mark at its start for no part of a
    // field, which one reading from the top keeps in the field.
    let marked = |from: usize| from > 0 && source[from..].starts_with('\u{feff}');
    if marked(last) || marked(end) {
        return None;
    }
    let mut reader = reader(&source.as_bytes()[last..], separator);
    let mut record = ByteRecord::new();
    let read = reader
        .read_byte_record(&mut record)
        .expect("a slice can be read");
    #[cfg(test)]
    ROWS_READ.with(|rows| rows.set(rows.get() + 1));
    if !read || last + byte(reader.position()) != end {
        return None;
    }
    Some(
        read_on(source, separator, max_cells, found)
            .map(|found| document(source, separator, found)),
    )
}

#[cfg(test)]
thread_local! {
    /// How many rows the parses on this thread read.
    static ROWS_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many rows the parses on this thread read, blank lines not counted.
#[cfg(test)]
pub(crate) fn rows_read() -> usize {
    ROWS_READ.with(std::cell::Cell::get)
}

/// `found` with the rows of `source` from the end of its last row on.
fn read_on(
    source: &str,
    separator: Separator,
    max_cells: usize,
    mut found: Found,
) -> Result<Found, TooWide> {
    let from = found.starts[found.starts.len() - 1];
    let mut reader = reader(&source.as_bytes()[from..], separator);
    let Found {
        starts,
        columns,
        cap,
        texts,
    } = &mut found;
    let mut record = ByteRecord::new();
    // Reading from a slice cannot fail, and records may differ in length.
    while reader
        .read_byte_record(&mut record)
        .expect("a slice can be read")
    {
        #[cfg(test)]
        ROWS_READ.with(|read| read.set(read.get() + 1));
        let rows = starts.len() - 1;
        let wider = (*columns).max(record.len());
        if cap.is_none() && (rows + 1).saturating_mul(wider) > max_cells {
            if rows == 0 {
                return Err(TooWide {
                    columns: record.len(),
                });
            }
            *cap = Some((rows, *columns));
        }
        *columns = wider;
        for (column, field) in record.iter().enumerate() {
            texts.add(rows, column, field);
        }
        starts.push(from + byte(reader.position()));
    }
    Ok(found)
}

/// The document of the table of `source` that `found` tells of.
fn document(source: &str, separator: Separator, found: Found) -> Document {
    let Found {
        starts,
        columns,
        cap,
        texts,
    } = found;
    if starts.len() == 1 {
        return Document::default();
    }
    // The table keeps the source up to the end of its last row.
    let end = starts[starts.len() - 1];
    let source_rows = Arc::from(&source[..end]);
    let rows = SourceRows::new(source_rows, separator, starts.into(), (columns, cap), texts);
    let left_out = rows.left_out();
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
    document
}

/// The byte `position` is at.
fn byte(position: &Position) -> usize {
    usize::try_from(position.byte()).expect("a position in a str fits a usize")
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

#[cfg(test)]
mod grown;
#[cfg(test)]
mod props;
#[cfg(test)]
mod tests;
