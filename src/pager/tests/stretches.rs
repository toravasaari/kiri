//! A large file in the pager: the stretches of it that the view is not near
//! are only measured, and are laid out when the view comes to them.

use super::*;
use crate::frontend::sim::Sim;

/// A file of `count` rows of code, `row 1` to `row count`, in `fn` items.
fn rows(count: usize) -> String {
    (1..=count)
        .map(|n| format!("fn row_{n}() {{ let at = {n}; }}\n"))
        .collect()
}

/// What the screen shows after each frame of `ran`.
fn texts(ran: &crate::frontend::sim::Ran) -> Vec<String> {
    let frames: Vec<String> = (ran.frames().iter())
        .map(|frame| (*frame).to_owned())
        .collect();
    replay(&frames)
        .iter()
        .map(|screen| crate::frontend::sim::plain(&screen_text(screen)))
        .collect()
}

#[test]
fn a_jump_into_a_stretch_that_is_only_measured_shows_its_lines() {
    for name in ["big.rs", "big.txt"] {
        let ran = Sim::new(60, 8)
            .file(name, &rows(4000))
            .keys(10, "/row_2000()\r")
            .keys(20, "G")
            .keys(30, "g")
            .keys(40, "q")
            .kiri(&[name]);
        let texts = texts(&ran);
        assert!(texts[0].contains("fn row_1()"), "{name}: {:?}", texts[0]);
        // The match is found though its stretch holds no text, and the view
        // goes to it: the frame shows the line and its neighbours.
        let found = (texts.iter()).find(|text| text.contains("fn row_2000()"));
        let found = found.unwrap_or_else(|| panic!("{name}: {texts:?}"));
        assert!(found.contains("1/1"), "{name}: {found:?}");
        assert!(found.contains("fn row_2001()"), "{name}: {found:?}");
        // No frame shows a row that holds nothing above a row of text.
        for text in &texts {
            let shown: Vec<&str> = text.lines().collect();
            let rows = &shown[..shown.len() - 1];
            let last = rows.iter().rposition(|row| !row.trim().is_empty());
            let rows = &rows[..last.map_or(0, |last| last + 1)];
            assert!(
                rows.iter().all(|row| row.contains("fn row_")),
                "{name}: {text:?}"
            );
        }
        assert!(
            texts.iter().any(|text| text.contains("fn row_4000()")),
            "{name}"
        );
        assert!(texts.last().unwrap().contains("fn row_1()"), "{name}");
    }
}

#[test]
fn paging_through_a_large_file_shows_every_row_in_order() {
    let mut sim = Sim::new(60, 8).file("big.rs", &rows(1800));
    for press in 0..260 {
        sim = sim.keys(10 + press, " ");
    }
    let ran = sim.keys(1000, "q").kiri(&["big.rs"]);
    let texts = texts(&ran);
    let mut next = 1;
    for text in &texts {
        for row in text.lines() {
            let Some(rest) = row.trim_start().strip_prefix("fn row_") else {
                continue;
            };
            let number: usize = rest[..rest.find('(').unwrap()].parse().unwrap();
            assert!(number <= next, "row {number} after {}: {text:?}", next - 1);
            next = next.max(number + 1);
        }
    }
    assert_eq!(next, 1801, "the last row shown");
}

#[test]
fn the_work_of_a_large_file_draws_no_frame_of_its_own() {
    let ran = Sim::new(60, 8)
        .file("big.rs", &rows(20000))
        .keys(10, "q")
        .kiri(&["big.rs"]);
    assert!(ran.frames().len() < 10, "{} frames", ran.frames().len());
}

#[test]
fn a_key_that_comes_in_two_pieces_is_one_key_while_the_layout_works() {
    let last = |pieces: &[(u64, &str)]| {
        let mut sim = Sim::new(60, 8).file("big.rs", &rows(20000));
        for (ms, keys) in pieces {
            sim = sim.keys(*ms, keys);
        }
        let ran = sim.keys(5000, "q").kiri(&["big.rs"]);
        texts(&ran).last().cloned().unwrap_or_default()
    };
    // The down arrow, whole and cut after its first two bytes.
    let whole = last(&[(1, "\x1b[B")]);
    assert_eq!(last(&[(0, "\x1b["), (1, "B")]), whole);
    assert!(
        whole.contains("fn row_2()") && !whole.contains("fn row_1()"),
        "{whole:?}"
    );
}

/// A CSV of `count` rows below its header: `1,row 1,3` to the last.
fn csv_rows(count: usize) -> String {
    let mut source = "n,name,value\n".to_owned();
    for n in 1..=count {
        source.push_str(&format!("{n},row {n},{}\n", n * 3));
    }
    source
}

#[test]
fn a_csv_of_a_million_rows_shows_its_first_frame_having_made_cells_of_few_of_them() {
    use crate::typeset::source_rows::cells_made;
    let source = csv_rows(1_000_000);
    let before = cells_made();
    let ran = Sim::new(60, 8)
        .file("big.csv", &source)
        .keys(10, "q")
        .kiri(&["big.csv"]);
    let made = cells_made() - before;
    // The first stretch, the last, and those near the view, of 128 rows
    // each: fewer than a thousand rows of three cells.
    assert!(made < 1000 * 3, "{made} cells");
    let texts = texts(&ran);
    assert!(texts[0].contains("row 1 "), "{:?}", texts[0]);
    assert!(!texts[0].contains("not shown"), "{:?}", texts[0]);
}

#[test]
fn paging_through_a_csv_of_a_million_rows_makes_cells_of_the_rows_near_the_view_only() {
    use crate::typeset::source_rows::cells_made;
    let source = csv_rows(1_000_000);
    let before = cells_made();
    let ran = Sim::new(60, 8)
        .file("big.csv", &source)
        .keys(10, "G")
        .keys(20, "\x1b[5~")
        .keys(30, "g")
        .keys(40, "/row 654321\r")
        .keys(50, "q")
        .kiri(&["big.csv"]);
    let made = cells_made() - before;
    // Each of the five views lays out the stretches within `NEAR` lines of
    // it and the background those within `AHEAD`: at most some 1300 rows a
    // view, 6500 in all, of three cells.
    assert!(made < 6500 * 3, "{made} cells");
    let texts = texts(&ran);
    // The widths are those of the first stretch's rows, so the numbers of
    // six digits and more wrap in their column.
    let shows = |row: &str| texts.iter().any(|text| text.contains(row));
    assert!(shows("│ 000 │ 1000000 │ 00    │\n  │ 0   │"), "{texts:?}");
    assert!(shows("╰─────┴─────────┴───────╯"), "{texts:?}");
    let found = texts.last().unwrap();
    assert!(
        found.contains("1/1") && found.contains("│ 654 │ row     │"),
        "{found:?}"
    );
    assert!(found.contains("│ 321 │ 654321  │"), "{found:?}");
}

/// A CSV of `count` rows with `needle` in the rows `at`.
fn csv_with_needles(count: usize, at: &[usize]) -> String {
    let mut source = "n,name\n".to_owned();
    for n in 1..=count {
        let name = match at.contains(&n) {
            true => format!("needle {n:04}"),
            false => format!("row {n:04}"),
        };
        source.push_str(&format!("{n:04},{name}\n"));
    }
    source
}

#[test]
fn a_search_finds_and_shows_the_matches_of_a_long_csv_with_n_and_shift_n() {
    let ran = Sim::new(60, 8)
        .file("long.csv", &csv_with_needles(3000, &[5, 1500, 2990]))
        .keys(10, "/needle\r")
        .keys(20, "n")
        .keys(30, "n")
        .keys(40, "N")
        .keys(50, "q")
        .kiri(&["long.csv"]);
    let texts = texts(&ran);
    let at = |shown: &str, count: &str| {
        (texts.iter())
            .position(|text| text.contains(shown) && text.contains(count))
            .unwrap_or_else(|| panic!("{shown} {count}: {texts:?}"))
    };
    let first = at("needle 0005", "1/3");
    let second = at("needle 1500", "2/3");
    let third = at("needle 2990", "3/3");
    let back =
        (texts.iter()).rposition(|text| text.contains("needle 1500") && text.contains("2/3"));
    assert!(first < second && second < third, "{texts:?}");
    assert!(back.is_some_and(|back| back > third), "{texts:?}");
}

#[test]
fn tab_goes_to_the_links_of_a_long_table_in_order() {
    let mut source = "| n | link |\n|---|---|\n".to_owned();
    for n in 1..=3000 {
        match n {
            5 | 1500 | 2990 => {
                source += &format!("| {n:04} | [go {n:04}](https://example.com/{n}) |\n")
            }
            _ => source += &format!("| {n:04} | row {n:04} |\n"),
        }
    }
    let caps = Capabilities {
        hyperlinks: true,
        ..Capabilities::ansi(60)
    };
    let ran = Sim::new(60, 8)
        .caps(caps)
        .file("long.md", &source)
        .keys(10, "\t")
        .keys(20, "\t")
        .keys(30, "\t")
        .keys(40, "q")
        .kiri(&["long.md"]);
    let texts = texts(&ran);
    let at = |shown: &str| {
        (texts.iter())
            .position(|text| text.contains(shown))
            .unwrap_or_else(|| panic!("{shown}: {texts:?}"))
    };
    let (first, second, third) = (at("go 0005"), at("go 1500"), at("go 2990"));
    assert!(first <= second && second < third, "{texts:?}");
}

#[test]
fn a_long_csv_that_grows_while_it_is_followed_shows_its_new_rows() {
    use crate::session::settings::{Follow, FollowJump};
    let ran = Sim::new(60, 8)
        .file("long.csv", &csv_with_needles(3000, &[]))
        .follow(Follow::Auto, FollowJump::First)
        .keys(10, "G")
        .write(500, "long.csv", &csv_with_needles(3002, &[3001]))
        .keys(2500, "/needle\r")
        .keys(3000, "q")
        .kiri(&["long.csv"])
        .quiet();
    assert!(matches!(ran.result, Some(Ok(()))), "{:?}", ran.result);
    let texts = ran.texts();
    // The new row is wider than the first stretch's rows, and wraps.
    let found =
        (texts.iter()).find(|text| text.contains("│ 3001 │ needle   │") && text.contains("1/1"));
    assert!(found.is_some(), "{texts:?}");
    assert!(
        texts.iter().any(|text| text.contains("3002 │ row 3002")),
        "{texts:?}"
    );
}
