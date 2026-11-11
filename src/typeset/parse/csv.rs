//! CSV and TSV source to document model: one table, the first row as its
//! header, as GitHub shows such a file.

use csv::{ReaderBuilder, StringRecord};

use crate::typeset::model::{Alignment, Block, Document, Inline, Place, Table};

/// What separates the fields of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
pub fn parse(source: &str, separator: Separator) -> Result<Document, TooWide> {
    parse_at_most(source, separator, MAX_CELLS)
}

/// The table in `source` with at most `max_cells` cells.
fn parse_at_most(
    source: &str,
    separator: Separator,
    max_cells: usize,
) -> Result<Document, TooWide> {
    let delimiter = match separator {
        Separator::Comma => b',',
        Separator::Tab => b'\t',
    };
    let mut reader = ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(source.as_bytes());
    let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    let mut columns = 0;
    // Rows past the limit are only counted, not kept.
    let mut left_out = 0;
    let mut record = StringRecord::new();
    // Text read from a `str` and split at ASCII delimiters stays UTF-8,
    // and records may differ in length.
    while reader
        .read_record(&mut record)
        .expect("a record of a str is UTF-8")
    {
        if left_out == 0 {
            let wider = columns.max(record.len());
            if (rows.len() + 1).saturating_mul(wider) <= max_cells {
                columns = wider;
                rows.push(record.iter().map(cell).collect());
                continue;
            }
            if rows.is_empty() {
                return Err(TooWide {
                    columns: record.len(),
                });
            }
        }
        left_out += 1;
    }
    if rows.is_empty() {
        return Ok(Document::default());
    }
    for row in &mut rows {
        row.resize(columns, Vec::new());
    }
    let header = rows.remove(0);
    let mut blocks = vec![Block::Table(Table {
        alignments: vec![Alignment::Left; columns],
        header,
        rows,
    })];
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

/// The cell holding `field`, its line breaks turned into spaces.
fn cell(field: &str) -> Vec<Inline> {
    if field.is_empty() {
        return Vec::new();
    }
    let text = field.replace("\r\n", " ").replace(['\r', '\n'], " ");
    vec![Inline::Text(text)]
}

#[cfg(test)]
mod tests;
