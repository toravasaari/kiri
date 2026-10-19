//! CSV and TSV source to document model: one table, the first row as its
//! header, as GitHub shows such a file.

use csv::ReaderBuilder;

use crate::model::{Alignment, Block, Document, Inline, Table};

/// What separates the fields of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    Comma,
    Tab,
}

/// The table in `source`. Cells are plain text; quoted fields follow
/// RFC 4180, and a newline inside one becomes a space. Blank lines are
/// skipped, short rows are padded with empty cells, and malformed quotes
/// are read as leniently as they can be rather than refused.
pub fn parse(source: &str, separator: Separator) -> Document {
    let delimiter = match separator {
        Separator::Comma => b',',
        Separator::Tab => b'\t',
    };
    let mut reader = ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(source.as_bytes());
    let mut rows: Vec<Vec<Vec<Inline>>> = reader
        .records()
        .map(|record| {
            // Text read from a `str` and split at ASCII delimiters stays
            // UTF-8, and records may differ in length.
            let record = record.expect("a record of a str is UTF-8");
            record.iter().map(cell).collect()
        })
        .collect();
    if rows.is_empty() {
        return Document::default();
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut rows {
        row.resize(columns, Vec::new());
    }
    let header = rows.remove(0);
    Document {
        blocks: vec![Block::Table(Table {
            alignments: vec![Alignment::Left; columns],
            header,
            rows,
        })],
    }
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
        parse(source, Separator::Comma)
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
            parse("a, b\tc\n1\t2, 3\r\n", Separator::Tab),
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
            parse("a\tb\n\"x\ty\"\t5\" floppy\n", Separator::Tab),
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
}
