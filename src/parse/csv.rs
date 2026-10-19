//! CSV and TSV source to document model: one table, the first row as its
//! header, as GitHub shows such a file.

use csv::{ReaderBuilder, StringRecord};

use crate::model::{Alignment, Block, Document, Inline, Table};

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
    Ok(Document { blocks })
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
mod tests {
    use super::*;
    use crate::model::{Alignment, Block, Inline, Table};

    fn cell(text: &str) -> Vec<Inline> {
        if text.is_empty() {
            Vec::new()
        } else {
            vec![Inline::Text(text.into())]
        }
    }

    /// The table `header` and `rows`, all columns left-aligned.
    fn table(header: &[&str], rows: &[&[&str]]) -> Document {
        Document {
            blocks: vec![Block::Table(Table {
                alignments: vec![Alignment::Left; header.len()],
                header: header.iter().map(|text| cell(text)).collect(),
                rows: rows
                    .iter()
                    .map(|row| row.iter().map(|text| cell(text)).collect())
                    .collect(),
            })],
        }
    }

    fn csv(source: &str) -> Document {
        parse(source, Separator::Comma).unwrap()
    }

    fn tsv(source: &str) -> Document {
        parse(source, Separator::Tab).unwrap()
    }

    #[test]
    fn the_first_row_is_the_header() {
        assert_eq!(
            csv("name,size\nkiri,small\nkitty,large\n"),
            table(
                &["name", "size"],
                &[&["kiri", "small"], &["kitty", "large"]]
            )
        );
    }

    #[test]
    fn a_header_alone_is_a_table_without_rows() {
        assert_eq!(csv("name,size"), table(&["name", "size"], &[]));
    }

    #[test]
    fn tabs_separate_tsv_and_commas_stay_in_cells() {
        assert_eq!(
            tsv("a, b\tc\n1\t2, 3\r\n"),
            table(&["a, b", "c"], &[&["1", "2, 3"]])
        );
    }

    #[test]
    fn quoted_fields_follow_rfc_4180() {
        assert_eq!(
            csv("text,note\n\"a, b\",\"say \"\"hi\"\"\"\n"),
            table(&["text", "note"], &[&["a, b", "say \"hi\""]])
        );
    }

    #[test]
    fn a_newline_in_a_quoted_field_becomes_a_space() {
        assert_eq!(
            csv("a\n\"one\ntwo\r\nthree\"\n"),
            table(&["a"], &[&["one two three"]])
        );
    }

    #[test]
    fn tsv_fields_are_quoted_as_in_csv() {
        assert_eq!(
            tsv("a\tb\n\"x\ty\"\t5\" floppy\n"),
            table(&["a", "b"], &[&["x\ty", "5\" floppy"]])
        );
    }

    #[test]
    fn cells_are_plain_text_not_markdown() {
        assert_eq!(
            csv("*a*,`b`\n[c](d),# e\n"),
            table(&["*a*", "`b`"], &[&["[c](d)", "# e"]])
        );
    }

    #[test]
    fn spaces_around_fields_are_kept() {
        assert_eq!(csv("a, b \n"), table(&["a", " b "], &[]));
    }

    #[test]
    fn short_rows_are_padded_to_the_longest_row() {
        assert_eq!(
            csv("a,b\n1\n1,2,3\n"),
            table(&["a", "b", ""], &[&["1", "", ""], &["1", "2", "3"]])
        );
    }

    #[test]
    fn blank_lines_are_skipped() {
        assert_eq!(
            csv("\na,b\n\n1,2\n\n\n3,4\n\n"),
            table(&["a", "b"], &[&["1", "2"], &["3", "4"]])
        );
    }

    #[test]
    fn an_empty_file_is_an_empty_document() {
        assert_eq!(csv(""), Document::default());
        assert_eq!(csv("\n\r\n"), Document::default());
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_first_cell() {
        assert_eq!(csv("\u{feff}a,b\n"), table(&["a", "b"], &[]));
    }

    #[test]
    fn malformed_quotes_are_read_leniently() {
        // A quote inside an unquoted field is a quote.
        assert_eq!(csv("a\"b\"c,d\n"), table(&["a\"b\"c", "d"], &[]));
        // An unclosed quote runs to the end of the file.
        assert_eq!(
            csv("a,b\n\"open,x\n1,2\n"),
            table(&["a", "b"], &[&["open,x 1,2 ", ""]])
        );
    }

    /// The table `header` and `rows` followed by `note`.
    fn with_note(header: &[&str], rows: &[&[&str]], note: &str) -> Document {
        let mut document = table(header, rows);
        document.blocks.push(Block::Note(note.into()));
        document
    }

    fn at_most(source: &str, max_cells: usize) -> Result<Document, TooWide> {
        parse_at_most(source, Separator::Comma, max_cells)
    }

    #[test]
    fn rows_past_the_cell_limit_are_left_out_with_a_note() {
        let source = "a,b\n1,2\n3,4\n5,6\n7,8\n";
        assert_eq!(
            at_most(source, 6),
            Ok(with_note(
                &["a", "b"],
                &[&["1", "2"], &["3", "4"]],
                "… 2 more rows not shown"
            ))
        );
        assert_eq!(
            at_most(source, 9),
            Ok(with_note(
                &["a", "b"],
                &[&["1", "2"], &["3", "4"], &["5", "6"]],
                "… 1 more row not shown"
            ))
        );
        assert_eq!(at_most(source, 10), Ok(csv(source)));
        assert!(!matches!(csv(source).blocks.last(), Some(Block::Note(_))));
    }

    #[test]
    fn padding_counts_towards_the_cell_limit() {
        // A wider row would pad the rows above it past the limit, and the
        // rows after it are left out too, so none is shown out of order.
        assert_eq!(
            at_most("a\n1\n2,3\n4\n", 5),
            Ok(with_note(&["a"], &[&["1"]], "… 2 more rows not shown"))
        );
    }

    #[test]
    fn a_header_alone_past_the_cell_limit_is_too_wide() {
        assert_eq!(at_most("a,b,c\n1\n", 2), Err(TooWide { columns: 3 }));
        assert_eq!(
            at_most("a,b,c\n1\n", 3),
            Ok(with_note(&["a", "b", "c"], &[], "… 1 more row not shown"))
        );
    }

    #[test]
    fn a_file_shows_at_most_max_cells() {
        let source = "x\n".repeat(MAX_CELLS + 1_000);
        let document = csv(&source);
        let [Block::Table(table), Block::Note(note)] = document.blocks.as_slice() else {
            panic!("{:?}", document.blocks.len());
        };
        assert_eq!(table.rows.len() + 1, MAX_CELLS);
        assert_eq!(note, "… 1\u{a0}000 more rows not shown");
    }

    #[test]
    fn counts_are_grouped_by_thousands() {
        assert_eq!(grouped(0, ' '), "0");
        assert_eq!(grouped(999, ' '), "999");
        assert_eq!(grouped(98_000, ' '), "98 000");
        assert_eq!(grouped(1_234_567, '\u{a0}'), "1\u{a0}234\u{a0}567");
        assert_eq!(
            TooWide { columns: 150_000 }.to_string(),
            "too wide to show: 150 000 columns, and kiri shows at most 100 000 cells"
        );
    }
}
