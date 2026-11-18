//! Tests of long tables in the pager's cell layout: a table of more rows
//! than a stretch is laid out in stretches of rows, the stretches between the
//! first and the last only measured, and its widths come from its first
//! stretch and what was measured over all its rows.

use crate::render::{Inputs, TableRows};
use crate::typeset::caps::Capabilities;
use crate::typeset::driver::{self, Content};
use crate::typeset::laid::Laid;
use crate::typeset::model::{Block, Document};
use crate::typeset::parse::csv::{self, Separator};
use crate::typeset::parse::parse;
use crate::typeset::position::Pos;
use crate::typeset::stretch::Tables;
use crate::typeset::style::{Line, text};

/// The inputs of the pager's cell layout, `width` wide, with `rows` told
/// apart as they say.
fn pager(width: u16, rows: TableRows) -> Inputs {
    Inputs {
        tables: Some(Tables::default()),
        table_rows: rows,
        ..Inputs::new(Capabilities::kitty(width))
    }
}

/// The cells of row `row` of a generated table of `rows` rows: short rows,
/// cells that wrap, a wide row in the middle, empty cells, letters of two
/// cells, a tab and a line break in a quoted field.
fn cells(row: usize, rows: usize) -> [String; 4] {
    match row {
        _ if row == rows / 2 => [
            format!("{row}"),
            "the widest row of all with words enough to wrap in any column".to_owned(),
            "日本語の文字は幅が二つ".to_owned(),
            "wide".to_owned(),
        ],
        _ if row % 9 == 4 => [
            format!("{row}"),
            String::new(),
            String::new(),
            String::new(),
        ],
        _ if row % 11 == 6 => [
            format!("{row}"),
            "words to wrap here and there".to_owned(),
            "a\tb".to_owned(),
            "x".to_owned(),
        ],
        _ => [
            format!("{row}"),
            format!("name {row}"),
            "Київ".to_owned(),
            format!("{}", row * 7),
        ],
    }
}

/// A CSV of `rows` body rows under a header.
fn csv_source(rows: usize) -> String {
    let mut source = "Number,Name,Place,Value\n".to_owned();
    for row in 0..rows {
        let [a, b, c, d] = cells(row, rows);
        // The tab goes in a quoted field, with a line break.
        let c = match c.contains('\t') {
            true => "\"a\tb\nc\"".to_owned(),
            false => c,
        };
        source.push_str(&format!("{a},{b},{c},{d}\n"));
    }
    source
}

/// A Markdown table of `rows` body rows, a paragraph before and after it,
/// some cells with inlines.
fn markdown_source(rows: usize) -> String {
    let mut source = "Before.\n\n| Number | Name | Place | Value |\n|---|:-:|---|--:|\n".to_owned();
    for row in 0..rows {
        let [a, b, c, d] = cells(row, rows);
        let b = match row % 13 {
            2 => format!("*{b}* and `code`"),
            _ => b,
        };
        source.push_str(&format!("| {a} | {b} | {} | {d} |\n", c.replace('\t', " ")));
    }
    source + "\nAfter.\n"
}

/// The documents of the tests: a CSV and a Markdown table of `rows` rows.
fn documents(rows: usize) -> Vec<(&'static str, Document)> {
    vec![
        (
            "csv",
            csv::parse(&csv_source(rows), Separator::Comma).unwrap(),
        ),
        ("markdown", parse(&markdown_source(rows))),
    ]
}

fn whole(document: &Document, inputs: &Inputs) -> Laid {
    driver::laid(Content::Document(document), inputs)
}

fn measured(document: &Document, inputs: &Inputs) -> Laid {
    driver::laid_measured(Content::Document(document), 0, inputs)
}

/// The text of `lines`, a line each.
fn texts(lines: &[Line]) -> Vec<String> {
    lines.iter().map(|line| text(&line.spans)).collect()
}

#[test]
fn a_long_table_laid_out_in_steps_is_the_table_laid_out_in_one() {
    use crate::steps::{Step, alike, assert_in_steps_of};
    for (what, document) in documents(300) {
        for rows in [TableRows::Zebra, TableRows::Lines, TableRows::Underline] {
            for width in [80, 30] {
                let inputs = pager(width, rows);
                let what = format!("{what} {rows:?} {width}");
                let steps = [Step::Piece, Step::Window(24), Step::Seam(131)];
                assert_in_steps_of(&what, Content::Document(&document), alike(&inputs), &steps);
            }
        }
    }
}

#[test]
fn a_long_table_is_cut_in_stretches_only_in_the_pager() {
    for (what, document) in documents(300) {
        let laid = whole(&document, &pager(80, TableRows::Zebra));
        let printed = whole(&document, &Inputs::new(Capabilities::kitty(80)));
        // Printed, the widest row widens its column; in the pager, the rows
        // of the first stretch choose the widths, and the wide row wraps.
        assert!(laid.len() > printed.len(), "{what}");
        let top = |laid: &Laid| texts(&laid.lines()[..6]);
        assert_ne!(top(&laid), top(&printed), "{what}");
        // One top border and one bottom border, whatever the stretches.
        let all = texts(laid.lines()).join("\n");
        assert_eq!(all.matches('╭').count(), 1, "{what}");
        assert_eq!(all.matches('╰').count(), 1, "{what}");
        assert_eq!(all.matches("Number").count(), 1, "{what}");
    }
}

#[test]
fn a_table_of_one_stretch_is_laid_out_as_in_print() {
    for (what, document) in documents(128) {
        let laid = whole(&document, &pager(80, TableRows::Zebra));
        let printed = whole(&document, &Inputs::new(Capabilities::kitty(80)));
        assert_eq!(laid.lines(), printed.lines(), "{what}");
    }
}

#[test]
fn the_first_stretch_is_laid_out_without_the_rows_after_it() {
    // In a table of 1000 rows whose row 500 is the widest, the first stretch
    // is the same as in the table without that row.
    let narrow = |source: &str| source.replace("the widest row of all, with words", "short");
    let csv_parse = |source: &str| csv::parse(source, Separator::Comma).unwrap();
    type Parse<'a> = &'a dyn Fn(&str) -> Document;
    let sources: [(&str, String, Parse); 2] = [
        ("csv", csv_source(1000), &csv_parse),
        ("markdown", markdown_source(1000), &parse),
    ];
    for (what, source, parse) in sources {
        let inputs = pager(100, TableRows::Zebra);
        let (wide, short) = (parse(&source), parse(&narrow(&source)));
        let (wide, short) = (measured(&wide, &inputs), measured(&short, &inputs));
        let first = wide.measured().first().map_or(0, |stretch| stretch.start);
        assert!(
            first > 100,
            "{what}: the first stretch ends at line {first}"
        );
        assert_eq!(wide.lines()[..first], short.lines()[..first], "{what}");
        // Printed, the row widens its column from the top.
        let printed = whole(&parse(&source), &Inputs::new(Capabilities::kitty(100)));
        let border = |laid: &Laid| {
            let lines = texts(laid.lines());
            let top = lines.iter().find(|line| line.contains('╭'));
            top.map_or(0, |line| line.chars().count())
        };
        assert!(border(&wide) < border(&printed), "{what}");
    }
}

#[test]
fn a_measured_table_has_the_lines_and_the_heights_of_the_whole_one() {
    for (what, document) in documents(700) {
        for rows in [TableRows::Zebra, TableRows::Lines, TableRows::Indent] {
            // Boxed, wrapping, and stacked.
            for width in [100, 40, 24] {
                let inputs = pager(width, rows);
                let what = format!("{what} {rows:?} {width}");
                let (part, all) = (measured(&document, &inputs), whole(&document, &inputs));
                assert_eq!(part.len(), all.len(), "{what}");
                assert_eq!(
                    part.height(&inputs.caps),
                    all.height(&inputs.caps),
                    "{what}"
                );
                let stretches = part.measured();
                assert!(stretches.len() >= 3, "{what}: {stretches:?}");
                for (at, (line, eager)) in part.lines().iter().zip(all.lines()).enumerate() {
                    assert_eq!(line.block, eager.block, "{what}: block of line {at}");
                }
                let first = stretches[0].start;
                assert_eq!(part.lines()[..first], all.lines()[..first], "{what}");
                let last = stretches[stretches.len() - 1].end;
                assert_eq!(part.lines()[last..], all.lines()[last..], "{what}");
            }
        }
    }
}

#[test]
fn a_stretch_measured_laid_let_go_and_laid_again_is_the_whole_one() {
    // Long enough that a stretch far from the view is let go.
    for (what, document) in documents(4000) {
        for width in [100, 40, 24] {
            let inputs = pager(width, TableRows::Lines);
            let content = Content::Document(&document);
            let (mut part, all) = (measured(&document, &inputs), whole(&document, &inputs));
            let lines = all.len();
            for top in [lines / 2, 40, lines - 50, lines / 2 + 3] {
                let near = top..(top + 40).min(lines);
                part.show(content, &inputs, near.clone());
                assert_eq!(
                    part.lines()[near.clone()],
                    all.lines()[near.clone()],
                    "{what} {width}: {near:?}"
                );
            }
            // Far from all of them, those were let go, and they come back.
            part.show(content, &inputs, lines - 10..lines);
            part.show(content, &inputs, 0..10);
            let middle = lines / 2 + 1000..lines / 2 + 1040;
            assert!(
                part.lines()[middle.clone()]
                    .iter()
                    .any(|line| line.measured.is_some())
            );
            part.show(content, &inputs, middle.clone());
            assert_eq!(part.lines()[middle.clone()], all.lines()[middle], "{what}");
        }
    }
}

#[test]
fn the_map_finds_the_rows_of_cells_in_a_stretch_that_is_only_measured() {
    for (what, document) in documents(700) {
        let inputs = pager(100, TableRows::Zebra);
        let (part, all) = (measured(&document, &inputs), whole(&document, &inputs));
        let path = match what {
            "csv" => vec![0],
            _ => vec![1],
        };
        let mut found = 0;
        for cell in (8..2800).step_by(37) {
            let range = Pos::text(0, &path, cell, 0)..Pos::text(0, &path, cell, 1);
            let Ok((_, eager)) = all.drawn_rows(&range) else {
                continue;
            };
            let Ok((_, rows)) = part.drawn_rows(&range) else {
                panic!("{what}: cell {cell}");
            };
            // The rows of the cell's row, which hold the cell's lines.
            assert!(
                rows.start <= eager.start && eager.end <= rows.end,
                "{what}: cell {cell}: {rows:?} against {eager:?}"
            );
            assert!(rows.len() <= 4, "{what}: cell {cell}: {rows:?}");
            assert_eq!(part.line_of(&range.start), Some(rows.start), "{what}");
            found += 1;
        }
        assert!(found > 50, "{what}: {found}");
        let last = part.len();
        assert_eq!(part.span_of(0..last), all.span_of(0..last), "{what}");
    }
}

#[test]
fn a_table_drawn_in_stretches_keeps_its_stripes_and_rules_at_the_seams() {
    // Stretches of an even and an odd number of rows: a wide table has
    // shorter ones. Every cell is as wide, so that the first stretch asks
    // for the widths that the whole table does.
    for (columns, width) in [(4, 100), (9, 160), (14, 160)] {
        let header: Vec<String> = (0..columns).map(|c| format!("h{c:04}")).collect();
        let mut source = header.join(",") + "\n";
        for row in 0..400 {
            let row: Vec<String> = (0..columns).map(|c| format!("{row:03}.{c:02}")).collect();
            source += &(row.join(",") + "\n");
        }
        let document = csv::parse(&source, Separator::Comma).unwrap();
        for rows in [TableRows::Zebra, TableRows::Lines, TableRows::Underline] {
            let caps = Capabilities {
                truecolor: true,
                ..Capabilities::kitty(width)
            };
            let inputs = Inputs {
                caps: caps.clone(),
                ..pager(width, rows)
            };
            let laid = whole(&document, &inputs);
            let printed = Inputs {
                caps,
                table_rows: rows,
                ..Inputs::new(Capabilities::kitty(width))
            };
            let printed = whole(&document, &printed);
            assert_eq!(laid.lines(), printed.lines(), "{columns} {rows:?}");
        }
    }
    let Block::Table(table) = &csv::parse("a,b\n1,2\n", Separator::Comma).unwrap().blocks[0] else {
        unreachable!()
    };
    assert_eq!(crate::typeset::stretch::rows_of(table, 128), 128);
}

#[test]
fn what_r_measures_of_a_long_table_gives_the_widths_of_all_its_rows() {
    use crate::typeset::driver::fitted;
    for (what, document) in documents(700) {
        for width in [100, 40] {
            let content = Content::Document(&document);
            let fitted = fitted(content, &pager(width, TableRows::Lines));
            let inputs = Inputs {
                tables: Some(fitted),
                ..pager(width, TableRows::Lines)
            };
            // Laid out in stretches with the widths of every row, the table is
            // the one printed whole.
            let printed = Inputs {
                table_rows: TableRows::Lines,
                ..Inputs::new(Capabilities::kitty(width))
            };
            let (laid, printed) = (whole(&document, &inputs), whole(&document, &printed));
            assert_eq!(laid.lines(), printed.lines(), "{what} {width}");
            assert_eq!(
                measured(&document, &inputs).len(),
                laid.len(),
                "{what} {width}"
            );
        }
    }
}

#[test]
fn what_was_measured_of_a_table_with_other_columns_is_not_taken() {
    use crate::typeset::driver::fitted;
    let wide = csv::parse(&csv_source(700), Separator::Comma).unwrap();
    let fitted = fitted(Content::Document(&wide), &pager(100, TableRows::Zebra));
    // The same table with a column more.
    let more = csv_source(700).replace('\n', ",x\n");
    let more = csv::parse(&more, Separator::Comma).unwrap();
    let carried = Inputs {
        tables: Some(fitted),
        ..pager(100, TableRows::Zebra)
    };
    let (carried, first) = (
        whole(&more, &carried),
        whole(&more, &pager(100, TableRows::Zebra)),
    );
    assert_eq!(carried.lines(), first.lines());
}

#[test]
fn a_stacked_long_table_with_styled_headers_has_the_lines_of_the_whole_one() {
    // Code keeps its spaces and a superscript is set small: their labels
    // wrap otherwise than their text alone.
    for header in ["`a  b`", "x<sup>23456</sup>", "**ab** `c d`"] {
        let mut source = format!("| {header} | Second | Third | Fourth |\n|---|---|---|---|\n");
        for row in 0..400 {
            let words: Vec<String> = (0..(row % 13))
                .map(|word| "abcdefghij"[..1 + (word * 7 + row) % 9].to_owned())
                .collect();
            let words = words.join(" ");
            source += &format!("| {words} | abcdefghijkl | abcdefghijkl | abcdefghijkl {row} |\n");
        }
        let document = parse(&source);
        for width in [22, 23, 24, 31] {
            let inputs = pager(width, TableRows::Off);
            let (part, all) = (measured(&document, &inputs), whole(&document, &inputs));
            assert_eq!(part.len(), all.len(), "{header} at {width}");
        }
    }
}
