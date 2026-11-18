//! CSV and TSV source to document model: one table, the first row as its
//! header, as GitHub shows such a file. The table holds the rows of the
//! source ([`SourceRows`]), which this finds the starts of.

use std::sync::Arc;

use csv::{ByteRecord, Position};

use crate::typeset::model::{Alignment, Block, Document, Place, Table};
pub use crate::typeset::source_rows::Separator;
use crate::typeset::source_rows::{SourceRows, reader};

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
    let mut reader = reader(source.as_bytes(), separator);
    // Where each row starts, and where the last of them ends.
    let mut starts = vec![0];
    let mut columns = 0;
    // The rows a capped view keeps, and the fields of the widest of them,
    // once a row does not fit in it.
    let mut cap = None;
    let mut record = ByteRecord::new();
    // Reading from a slice cannot fail, and records may differ in length.
    while reader
        .read_byte_record(&mut record)
        .expect("a slice can be read")
    {
        let rows = starts.len() - 1;
        let wider = columns.max(record.len());
        if cap.is_none() && (rows + 1).saturating_mul(wider) > max_cells {
            if rows == 0 {
                return Err(TooWide {
                    columns: record.len(),
                });
            }
            cap = Some((rows, columns));
        }
        columns = wider;
        starts.push(byte(reader.position()));
    }
    if starts.len() == 1 {
        return Ok(Document::default());
    }
    // The table keeps the source up to the end of its last row.
    let end = starts[starts.len() - 1];
    let source_rows = Arc::from(&source[..end]);
    let rows = SourceRows::new(source_rows, separator, starts.into(), (columns, cap));
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
    Ok(document)
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
mod props;
#[cfg(test)]
mod tests;
