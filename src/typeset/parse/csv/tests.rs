use crate::typeset::model::{Alignment, Block, Inline, Place, Table};
use crate::typeset::parse::csv::*;
use crate::typeset::text::{DocumentText, Piece, pieces};

pub(super) fn cell(text: &str) -> Vec<Inline> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Inline::Text(text.into())]
    }
}

/// The table `header` and `rows`, all columns left-aligned.
fn table(header: &[&str], rows: &[&[&str]]) -> Document {
    Document::new(vec![Block::Table(Table::new(
        vec![Alignment::Left; header.len()],
        header.iter().map(|text| cell(text)).collect(),
        rows.iter()
            .map(|row| row.iter().map(|text| cell(text)).collect())
            .collect(),
    ))])
}

/// `document` with the cells of its tables made, read through the table's
/// access, so that it compares with tables of parsed cells.
fn made(mut document: Document) -> Document {
    for block in &mut document.blocks {
        if let Block::Table(table) = block {
            let rows = table.rows(0..table.row_count()).into_owned();
            *table = Table::new(table.alignments.clone(), table.header().into_owned(), rows);
        }
    }
    document
}

fn csv(source: &str) -> Document {
    made(parse(source, Separator::Comma).unwrap())
}

fn tsv(source: &str) -> Document {
    made(parse(source, Separator::Tab).unwrap())
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
    parse_at_most(source, Separator::Comma, max_cells).map(made)
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
    assert_eq!(table.row_count() + 1, MAX_CELLS);
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

/// The header and the rows of the table of `source` read the first way: each
/// record the `csv` crate reads from the whole source becomes a row of cells
/// at once, padded to the widest. What the table of `parse` gives, however
/// it keeps its rows, must be this.
pub(super) fn read_whole(source: &str, separator: Separator) -> Vec<Vec<Vec<Inline>>> {
    let delimiter = match separator {
        Separator::Comma => b',',
        Separator::Tab => b'\t',
    };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(source.as_bytes());
    let mut rows: Vec<Vec<Vec<Inline>>> = (reader.records())
        .map(|record| {
            let record = record.unwrap();
            let text = |field: &str| field.replace("\r\n", " ").replace(['\r', '\n'], " ");
            record.iter().map(|field| cell(&text(field))).collect()
        })
        .collect();
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut rows {
        row.resize(columns, Vec::new());
    }
    rows
}

/// The header and the rows of `table`, as its access gives them.
pub(super) fn rows_of(table: &Table) -> Vec<Vec<Vec<Inline>>> {
    std::iter::once(table.header().into_owned())
        .chain(table.rows(0..table.row_count()).into_owned())
        .collect()
}

/// The texts the document's text gives for the table of `document`, each
/// asked for by its number.
pub(super) fn texts_by_number(document: &Document) -> Vec<Piece> {
    let text = DocumentText::new([document]);
    let texts = text.texts(0, &[0]);
    (0..texts.len())
        .map(|number| texts.get(number).unwrap().into_owned())
        .collect()
}

/// Whether the table of `source` and its texts are what reading the whole
/// source at once gives.
pub(super) fn read_as_before(source: &str, separator: Separator) -> Result<(), String> {
    let whole = read_whole(source, separator);
    let document = parse(source, separator).map_err(|error| error.to_string())?;
    let Some(Block::Table(table)) = document.blocks.first() else {
        return match whole.is_empty() {
            true => Ok(()),
            false => Err(format!("{source:?}: no table, but {whole:?}")),
        };
    };
    if rows_of(table) != whole {
        return Err(format!("{source:?}: {:?} is not {whole:?}", rows_of(table)));
    }
    let (header, rows) = whole.split_first().unwrap();
    let alignments = vec![Alignment::Left; header.len()];
    let before = pieces(&Block::Table(Table::new(
        alignments,
        header.clone(),
        rows.to_vec(),
    )));
    let texts = texts_by_number(&document);
    match texts == before {
        true => Ok(()),
        false => Err(format!("{source:?}: texts {texts:?} are not {before:?}")),
    }
}

/// Sources with what a CSV can hold: quotes, separators and line breaks in
/// fields, blank lines, short and long rows, and the edges of a file.
const SOURCES: &[&str] = &[
    "",
    "\n\r\n",
    "a,b",
    "a,b\n",
    "a,b\n1,2",
    "a,b\r\n1,2\r3,4\n5,6",
    "text,note\n\"a, b\",\"say \"\"hi\"\"\"\n",
    "a\n\"one\ntwo\r\nthree\"\n",
    "a\n\"one\r\n\"\n2\n",
    "\na,b\n\n1,2\n\n\n3,4\n\n",
    "a,b\n1\n1,2,3\n",
    "a,b,c,d\n1\n\n1,2\n",
    "a,b,\n1,2,\n",
    "\u{feff}a,b\n1,2\n",
    "a\n\u{feff}b\n\u{feff}c,d",
    "a\"b\"c,d\n",
    "a,b\n\"open,x\n1,2\n",
    "a,\"b\"\"\"\n\"\"\n,\n",
    "city,note\nSäkylä,\"naïve, ☕\"\nСофія,\"ї\nґ\"",
    "a\tb\n\"x\ty\"\t5\" floppy\n",
];

#[test]
fn a_table_and_its_texts_are_what_reading_the_whole_source_gives() {
    for source in SOURCES {
        for separator in [Separator::Comma, Separator::Tab] {
            if let Err(error) = read_as_before(source, separator) {
                panic!("{separator:?}: {error}");
            }
        }
    }
}

/// A CSV of a header and `rows` rows of three cells, the last row holding
/// `last` in its second cell.
fn long_csv(rows: usize, last: &str) -> String {
    let mut source = String::from("id,name,note\n");
    for row in 1..rows {
        source.push_str(&format!("{row},\"name, {row}\",plain\n"));
    }
    source.push_str(&format!("{rows},{last},\"two\nlines\"\n"));
    source
}

/// The table of `document`.
fn table_of(document: &Document) -> &Table {
    match document.blocks.first() {
        Some(Block::Table(table)) => table,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_table_of_a_csv_makes_cells_only_of_the_rows_asked_for() {
    let source = long_csv(1000, "last");
    let before = cells_made();
    let document = parse(&source, Separator::Comma).unwrap();
    assert_eq!(cells_made() - before, 0, "the load made cells");
    let table = table_of(&document);
    assert_eq!((table.row_count(), table.alignments.len()), (1000, 3));
    let rows = table.rows(9..12);
    assert_eq!(cells_made() - before, 9, "three rows of three cells");
    assert_eq!(rows[0], [cell("10"), cell("name, 10"), cell("plain")]);
    assert_eq!(rows[2][0], cell("12"));
    assert_eq!(
        table.rows(999..2000).as_ref(),
        [[cell("1000"), cell("last"), cell("two lines")]]
    );
    assert_eq!(
        table.header().as_ref(),
        [cell("id"), cell("name"), cell("note")]
    );
    assert_eq!(cells_made() - before, 15);
}

#[test]
fn a_layout_of_the_whole_table_makes_each_cell_once() {
    use crate::render::Inputs;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::driver::{Content, content};
    let document = parse(&long_csv(300, "last"), Separator::Comma).unwrap();
    let before = cells_made();
    let stream = content(
        Content::Document(&document),
        &Inputs::new(Capabilities::kitty(80)),
    );
    assert_eq!(cells_made() - before, 301 * 3);
    assert!(stream.lines.len() > 300);
}

#[test]
fn a_search_finds_a_term_in_the_last_row_without_making_a_cell() {
    let rows = 20_000;
    let before = cells_made();
    let document = parse(&long_csv(rows, "a needle"), Separator::Comma).unwrap();
    let text = DocumentText::new([&document]);
    let found = text.find("needle", true);
    assert_eq!(cells_made() - before, 0);
    // The second cell of the last row, after the header and the rows.
    let number = rows * 3 + 1;
    assert_eq!(
        found,
        [crate::typeset::position::Pos::text(0, &[0], number, 2)
            ..crate::typeset::position::Pos::text(0, &[0], number, 8)]
    );
    assert_eq!(text.text(&found[0]), "needle");
    assert_eq!(cells_made() - before, 0);
}

#[test]
fn the_text_of_a_table_answers_for_a_cell_by_its_number_from_the_source() {
    let rows = 5_000;
    let before = cells_made();
    let document = parse(&long_csv(rows, "last"), Separator::Comma).unwrap();
    let text = DocumentText::new([&document]);
    let texts = text.texts(0, &[0]);
    assert_eq!(texts.len(), (rows + 1) * 3);
    let piece = |text: &str, before| Piece {
        before,
        text: text.to_owned().into(),
    };
    use crate::typeset::text::Before;
    assert_eq!(
        texts.get(4).unwrap().as_ref(),
        &piece("name, 1", Before::Cell)
    );
    assert_eq!(
        texts.get(3 * 77).unwrap().as_ref(),
        &piece("77", Before::Line)
    );
    let last = piece("two lines", Before::Cell);
    assert_eq!(texts.last().unwrap().as_ref(), &last);
    assert_eq!(texts.get((rows + 1) * 3), None);
    assert_eq!(cells_made() - before, 0);
}

#[test]
fn tables_of_one_source_are_equal_and_a_changed_byte_gives_another_hash() {
    let source = long_csv(2_000, "last");
    let changed = source.replace("1999,", "1998,");
    let before = cells_made();
    let [one, again, other] =
        [&source, &source, &changed].map(|source| parse(source, Separator::Comma).unwrap());
    assert_eq!(one, again);
    assert_ne!(one, other);
    let hashes = [&one, &again, &other].map(|document| document.hashes().block(&[0]));
    assert_eq!(hashes[0], hashes[1]);
    assert_ne!(hashes[0], hashes[2]);
    assert_ne!(
        parse(&source, Separator::Comma).unwrap(),
        parse(&source, Separator::Tab).unwrap()
    );
    assert_eq!(cells_made() - before, 0);
}
