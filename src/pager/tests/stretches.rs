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
