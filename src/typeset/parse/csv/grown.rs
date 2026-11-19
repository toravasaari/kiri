//! A CSV that grew at its end is parsed on after its last row: what the
//! parse finds is what a parse of the whole finds, and it reads only the
//! old last row, to see that it stayed, and the new rows.

use super::{Block, Document, MAX_CELLS, Separator, parse_at_most, parse_grown_at_most, rows_read};

/// The rows of a table of `document`, and the blocks after it, as a parse
/// found them, to compare two parses by.
fn found(document: &Document) -> String {
    let mut out = String::new();
    for block in &document.blocks {
        match block {
            Block::Table(table) => {
                let rows = table.source().expect("a table of a source");
                out.push_str(&format!("{:?} {:?}\n", rows, rows.parts()));
            }
            other => out.push_str(&format!("{other:?}\n")),
        }
    }
    out.push_str(&format!("{:?}", document.places));
    out
}

/// Parses `whole` as grown from `before`, with at most `max_cells` cells,
/// and checks that it is what parsing the whole gives, when it is not
/// parsed whole; whether it was carried on.
pub(super) fn check_grown(
    before: &str,
    whole: &str,
    separator: Separator,
    max_cells: usize,
) -> Result<bool, String> {
    let Ok(old) = parse_at_most(before, separator, max_cells) else {
        return Ok(false);
    };
    let fresh = parse_at_most(whole, separator, max_cells);
    match parse_grown_at_most(&old, whole, separator, max_cells) {
        None => Ok(false),
        Some(grown) => match (grown, fresh) {
            (Ok(grown), Ok(fresh)) if found(&grown) == found(&fresh) && grown == fresh => Ok(true),
            (Err(grown), Err(fresh)) if grown == fresh => Ok(true),
            (grown, fresh) => Err(format!(
                "{before:?} grown to {whole:?}:\n{:?}\nagainst\n{:?}",
                grown.as_ref().map(found),
                fresh.as_ref().map(found)
            )),
        },
    }
}

/// `count` rows of a CSV after a header, numbered from `from`.
fn rows(from: usize, count: usize) -> String {
    (from..from + count)
        .map(|n| format!("{n},name {n},\"a note, {n}\"\n"))
        .collect()
}

#[test]
fn a_csv_with_rows_appended_reads_only_the_rows_from_its_old_last_on() {
    let before = format!("id,name,note\n{}", rows(1, 20_000));
    let old = parse_at_most(&before, Separator::Comma, MAX_CELLS).unwrap();
    for added in [1, 3, 10_000] {
        let whole = format!("{before}{}", rows(20_001, added));
        let read = rows_read();
        let grown = parse_grown_at_most(&old, &whole, Separator::Comma, MAX_CELLS)
            .expect("grown")
            .unwrap();
        // The old last row, read again to see that it stayed, and the new.
        assert_eq!(rows_read() - read, added + 1, "{added} rows appended");
        let fresh = parse_at_most(&whole, Separator::Comma, MAX_CELLS).unwrap();
        assert_eq!(found(&grown), found(&fresh));
    }
}

#[test]
fn a_csv_grown_anywhere_is_parsed_as_the_whole() {
    // A cut inside a quoted field, inside a line break of two bytes, at a
    // blank line, in a short row that gets longer, before a wider row.
    let whole = "id,name\r\n1,\"a\r\nb\",x\n\n2,c\n3,\"d,\"\"e\"\"\",f,g\n4\n";
    for cut in (0..=whole.len()).filter(|&cut| whole.is_char_boundary(cut)) {
        for separator in [Separator::Comma, Separator::Tab] {
            check_grown(&whole[..cut], whole, separator, MAX_CELLS).unwrap();
        }
    }
    // Found by the property test: a quoted field that a tab comes into,
    // which is then measured otherwise, and a row that starts with a byte
    // order mark, which a reader starting there would drop.
    for (before, whole) in [
        ("\"\n", "\"\n\t"),
        ("\naaaaa\n", "\naaaaa\n\u{feff}\"aa\naaaaaaaaaaa"),
    ] {
        check_grown(before, whole, Separator::Comma, MAX_CELLS).unwrap();
    }
}

#[test]
fn a_csv_whose_start_changed_is_not_parsed_on() {
    let before = format!("id,name,note\n{}", rows(1, 300));
    let old = parse_at_most(&before, Separator::Comma, MAX_CELLS).unwrap();
    let changed = [
        // A byte in the middle.
        before.replacen("name 150", "name 15X", 1),
        // A line removed at the top.
        before.replacen("1,name 1,\"a note, 1\"\n", "", 1),
        // Written again from its start, as a rotated log is.
        format!("id,name,note\n{}", rows(1, 2)),
        // Shorter.
        before[..before.len() - 10].to_owned(),
    ];
    for whole in changed {
        assert!(
            parse_grown_at_most(&old, &whole, Separator::Comma, MAX_CELLS).is_none(),
            "{:?}",
            &whole[..40]
        );
    }
    // Another separator is another table.
    assert!(parse_grown_at_most(&old, &before, Separator::Tab, MAX_CELLS).is_none());
}

#[test]
fn a_csv_that_comes_to_its_cap_while_it_grows_has_the_cap_of_the_whole() {
    // 4 cells a row: a capped view of 40 cells keeps 10 rows.
    let mut whole = "a,b,c,d\n".to_owned();
    for n in 0..30 {
        let row = match n % 7 {
            3 => format!("{n},x\n"),
            5 => format!("{n},x,y,z,w,v\n"),
            _ => format!("{n},x,y,z\n"),
        };
        whole.push_str(&row);
    }
    for cut in 0..=whole.len() {
        check_grown(&whole[..cut], &whole, Separator::Comma, 40).unwrap();
    }
}
