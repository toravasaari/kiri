use crate::typeset::model::{Alignment, Block, Inline, Place, Table};
use crate::typeset::parse::csv::*;

fn cell(text: &str) -> Vec<Inline> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Inline::Text(text.into())]
    }
}

/// The table `header` and `rows`, all columns left-aligned.
fn table(header: &[&str], rows: &[&[&str]]) -> Document {
    Document::new(vec![Block::Table(Table {
        alignments: vec![Alignment::Left; header.len()],
        header: header.iter().map(|text| cell(text)).collect(),
        rows: rows
            .iter()
            .map(|row| row.iter().map(|text| cell(text)).collect())
            .collect(),
    })])
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
fn rows_end_at_any_line_break() {
    assert_eq!(
        csv("a,b\r\n1,2\r3,4\n5,6"),
        table(&["a", "b"], &[&["1", "2"], &["3", "4"], &["5", "6"]])
    );
}

#[test]
fn a_trailing_separator_ends_with_an_empty_cell() {
    assert_eq!(
        csv("a,b,\n1,2,\n"),
        table(&["a", "b", ""], &[&["1", "2", ""]])
    );
}

#[test]
fn text_beyond_ascii_is_kept() {
    assert_eq!(
        csv("city,note\nSäkylä,\"naïve, ☕\"\n"),
        table(&["city", "note"], &[&["Säkylä", "naïve, ☕"]])
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
fn the_table_starts_on_the_first_line_and_its_note_on_none() {
    let source = "a\n1\n2\n";
    let document = at_most(source, 2).unwrap();
    assert!(matches!(document.blocks[1], Block::Note(_)));
    assert_eq!(document.places.line(&[0]), Some(1));
    assert_eq!(document.places.get(&[0]), Some(&Place::file(source.len())));
    assert_eq!(document.places.get(&[1]), None);
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
