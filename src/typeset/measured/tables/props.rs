//! Property test of a long CSV's measured stretches: whatever its fields,
//! a stretch has as many lines measured as it has laid out, and the lines
//! laid out are those of the whole layout.

use proptest::prelude::*;

use super::*;
use crate::testing::props_config;

/// Fields that reach the cases of measuring and wrapping: empty and space
/// only, leading and trailing spaces, wide, combining and joined letters,
/// no-break, zero-width and ideographic spaces, a soft hyphen, line
/// separators, controls, long words, and quoted fields with a tab or a line
/// break.
const FIELDS: &[&str] = &[
    "",
    " ",
    "   ",
    "a",
    "ab cd",
    "  lead",
    "trail  ",
    "a  b",
    "日本語",
    "日本語の文字は幅が二つ",
    "😀",
    "👨\u{200d}👩\u{200d}👧",
    "e\u{301}",
    "\u{a0}",
    "a\u{a0}b",
    "a\u{200b}b",
    "longwordwithoutspaces1234567890",
    "x y z w v u t s r q p",
    "\u{2028}",
    "a\u{2028}b",
    "\"q\tb\"",
    "\"l1\nl2\"",
    "Київ",
    "0",
    "12345",
    "user1000@example.com",
    "🇫🇮",
    "-",
    "https://example.com/a/very/long/path?with=query",
    "word, word; word.",
    "\u{ad}",
    "a\u{ad}b",
    "\u{3000}",
    "a\u{3000}b",
    "\u{feff}x",
    "\u{202e}abc",
    "❤\u{fe0f}",
    "ｱ",
    "\u{7f}",
];

/// A field of up to three pieces of `FIELDS`, quoted when one of them is.
fn field() -> impl Strategy<Value = String> {
    proptest::collection::vec(0..FIELDS.len(), 0..4).prop_map(|pieces| {
        let mut quoted = false;
        let words: Vec<&str> = (pieces.iter())
            .map(|&at| match FIELDS[at].strip_prefix('"') {
                Some(inner) => {
                    quoted = true;
                    inner.trim_end_matches('"')
                }
                None => FIELDS[at],
            })
            .collect();
        let text = words.join(" ").replace(',', ";");
        match quoted {
            true => format!("\"{text}\""),
            false => text,
        }
    })
}

/// A CSV of a header of `columns` fields and 140 to 400 rows, some short.
/// The header's fields are as awkward as the others, as a stacked table's
/// labels are made of them.
fn source() -> impl Strategy<Value = String> {
    (1usize..8).prop_flat_map(|columns| {
        let header = proptest::collection::vec(field(), columns);
        let row = proptest::collection::vec(field(), 1..=columns);
        (header, proptest::collection::vec(row, 140..400)).prop_map(|(header, rows)| {
            let mut source = header.join(",") + "\n";
            for row in rows {
                source += &(row.join(",") + "\n");
            }
            source
        })
    })
}

fn rows_style() -> impl Strategy<Value = TableRows> {
    prop_oneof![
        Just(TableRows::Off),
        Just(TableRows::Zebra),
        Just(TableRows::Lines),
        Just(TableRows::Indent),
        Just(TableRows::Underline),
    ]
}

proptest! {
    #![proptest_config(props_config())]

    #[test]
    fn a_long_csv_is_measured_as_it_is_laid_out(
        source in source(),
        rows in rows_style(),
        width in 6u16..125,
        scanned in any::<bool>(),
    ) {
        let document = csv::parse(&source, Separator::Comma).unwrap();
        let mut inputs = pager(width, rows);
        // Half with the widths of all the rows, as the pager opens a CSV,
        // and half with those of the first stretch.
        if scanned {
            let mut tables = Tables::default();
            driver::scanned(Content::Document(&document), &inputs, &mut tables);
            inputs.tables = Some(tables);
        }
        let (mut part, all) = (measured(&document, &inputs), whole(&document, &inputs));
        prop_assert_eq!(part.len(), all.len());
        let lines = all.len();
        part.show(Content::Document(&document), &inputs, 0..lines);
        for (at, (line, eager)) in part.lines().iter().zip(all.lines()).enumerate() {
            prop_assert!(line.measured.is_some() || line == eager, "line {}", at);
        }
    }
}
