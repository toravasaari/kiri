//! The mouse on the contents bar: the wheel, a click, a drag of the
//! border. The window is 120 × 12, so the bar is 30 columns wide with 11
//! rows, and the document starts at column 31.

use super::sidebar::{SOURCE, bar_of, click, document_of, edges, many, ran, status_of};
use super::*;
use crate::session::sidebar::Visibility;

/// A notch of the wheel down at `column` and `row`, from 1.
fn wheel_down(column: usize, row: usize) -> Vec<u8> {
    format!("\x1b[<65;{column};{row}M").into_bytes()
}

/// A notch of the wheel up at `column` and `row`, from 1.
fn wheel_up(column: usize, row: usize) -> Vec<u8> {
    format!("\x1b[<64;{column};{row}M").into_bytes()
}

/// The first entry of the bar on `seen`, as the number in its `H` name.
fn first_entry(seen: &Seen) -> usize {
    let bar = bar_of(seen);
    let row = bar.lines().next().unwrap();
    let at = row.find('H').unwrap_or_else(|| panic!("an entry in {row}"));
    row[at + 1..at + 3].trim().parse().unwrap()
}

#[test]
fn the_wheel_over_the_bar_scrolls_the_bar_and_not_the_document() {
    on_both(|| {
        let source = many(30);
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        let wheel = wheel_down(5, 3);
        let run = ran(&source, 120, Visibility::Auto, &[&wheel, b"q"]);
        let seen = &run.seen()[1];
        // Three rows a notch, as the wheel scrolls the document.
        assert_eq!(first_entry(seen), 3, "{}", bar_of(seen));
        assert_eq!(document_of(seen), document_of(&top.seen()[0]));
    });
}

#[test]
fn the_wheel_over_the_document_scrolls_the_document_as_before_with_the_bar_there() {
    on_both(|| {
        let source = many(30);
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        let wheel = wheel_down(60, 3);
        let run = ran(&source, 120, Visibility::Auto, &[&wheel, b"q"]);
        let seen = &run.seen()[1];
        assert_ne!(document_of(seen), document_of(&top.seen()[0]));
        assert_eq!(first_entry(seen), 0, "{}", bar_of(seen));
    });
}

#[test]
fn the_wheel_over_the_bar_stops_at_its_first_and_last_row() {
    on_both(|| {
        let source = many(30);
        let (up, down) = (wheel_up(5, 3), wheel_down(5, 3));
        let mut keys: Vec<&[u8]> = vec![&up];
        keys.extend(vec![&down[..]; 20]);
        keys.push(b"q");
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let screens = run.seen();
        // Up at the top draws nothing new: the first change is the first
        // notch down, to row 3.
        assert_eq!(first_entry(&screens[0]), 0);
        assert_eq!(first_entry(&screens[1]), 3, "{}", bar_of(&screens[1]));
        // 30 entries in 11 rows: the last view starts at entry 19.
        let last = &screens[screens.len() - 1];
        assert_eq!(first_entry(last), 19, "{}", bar_of(last));
    });
}

#[test]
fn the_wheel_over_the_bar_does_not_move_the_selection_or_the_mark() {
    on_both(|| {
        let source = many(30);
        let wheel = wheel_down(5, 3);
        // `t` selects the heading the reader is in, H00; scrolling the bar
        // leaves it, and Enter then jumps to it and not to what is in view.
        let run = ran(&source, 120, Visibility::Auto, &[b"t", &wheel, b"\r", b"q"]);
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        let last = run.seen().last().unwrap();
        assert_eq!(document_of(last), document_of(&top.seen()[0]));
        // The bar did not follow the selection back either: the wheel's
        // rows stay while the bar has the focus.
        let shown = &run.seen()[2];
        assert_eq!(first_entry(shown), 3, "{}", bar_of(shown));
    });
}

#[test]
fn a_move_of_the_document_brings_the_bar_back_to_the_current_heading() {
    on_both(|| {
        let source = many(30);
        let wheel = wheel_down(5, 3);
        let run = ran(
            &source,
            120,
            Visibility::Auto,
            &[&wheel, &wheel, b"j", b"q"],
        );
        let screens = run.seen();
        assert_eq!(first_entry(&screens[2]), 6, "{}", bar_of(&screens[2]));
        assert_eq!(first_entry(&screens[3]), 0, "{}", bar_of(&screens[3]));
    });
}

#[test]
fn a_heading_scrolled_out_of_the_bar_comes_back_when_the_document_moves() {
    on_both(|| {
        let source = many(30);
        // The document is at H00; the bar scrolled to the end, and then the
        // document goes to the end: the bar shows the last heading.
        let wheel = wheel_down(5, 3);
        let mut keys: Vec<&[u8]> = vec![&wheel; 10];
        keys.extend([&b"G"[..], b"q"]);
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let screens = run.seen();
        let last = &screens[screens.len() - 1];
        assert!(bar_of(last).contains("H29"), "{}", bar_of(last));
        assert_eq!(first_entry(last), 19, "{}", bar_of(last));
        // And the document's place counts: a jump to the top brings H00 back.
        keys.insert(keys.len() - 1, b"g");
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let last = run.seen().last().unwrap().clone();
        assert_eq!(first_entry(&last), 0, "{}", bar_of(&last));
    });
}

#[test]
fn without_a_bar_the_wheel_scrolls_the_document_wherever_it_is() {
    on_both(|| {
        let source = many(30);
        let top = ran(&source, 120, Visibility::Closed, &[b"q"]);
        let wheel = wheel_down(5, 3);
        let run = ran(&source, 120, Visibility::Closed, &[&wheel, b"q"]);
        assert_ne!(
            run.seen()[1].text(),
            top.seen()[0].text(),
            "the document scrolled"
        );
    });
}

#[test]
fn the_wheel_on_the_status_line_under_the_bar_scrolls_the_document() {
    on_both(|| {
        let source = many(30);
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        let wheel = wheel_down(5, 12);
        let run = ran(&source, 120, Visibility::Auto, &[&wheel, b"q"]);
        let seen = &run.seen()[1];
        assert_ne!(document_of(seen), document_of(&top.seen()[0]));
        assert_eq!(first_entry(seen), 0, "{}", bar_of(seen));
    });
}

#[test]
fn t_and_the_keys_of_the_bar_bring_the_selection_into_view() {
    on_both(|| {
        let source = many(30);
        let wheel = wheel_down(5, 3);
        // `t` selects the entry the reader is in, which the scroll had put
        // out of view: the bar shows it.
        let run = ran(
            &source,
            120,
            Visibility::Auto,
            &[&wheel, &wheel, b"t", b"q"],
        );
        let screens = run.seen();
        assert_eq!(first_entry(&screens[2]), 6, "{}", bar_of(&screens[2]));
        assert_eq!(first_entry(&screens[3]), 0, "{}", bar_of(&screens[3]));
        // And so does a key that moves the selection.
        let run = ran(
            &source,
            120,
            Visibility::Auto,
            &[b"t", &wheel, &wheel, b"j", b"q"],
        );
        let screens = run.seen();
        assert_eq!(first_entry(&screens[3]), 6, "{}", bar_of(&screens[3]));
        assert_eq!(first_entry(&screens[4]), 0, "{}", bar_of(&screens[4]));
    });
}

/// The text of the first row of the document's cells in `seen`.
fn first_line(seen: &Seen) -> String {
    document_of(seen)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

#[test]
fn a_click_on_a_heading_row_of_the_bar_jumps_there() {
    on_both(|| {
        let source = many(30);
        // Row 6 is H05, whose section is far below the first screen.
        let at = click(8, 6);
        let run = ran(&source, 120, Visibility::Auto, &[&at, b"q"]);
        let seen = run.seen().last().unwrap();
        assert_eq!(first_line(seen), "H05", "{}", seen.text());
        // The bar marks it, and the status line says nothing of the jump.
        assert!(edges(seen).contains(&6), "{}", bar_of(seen));
        assert!(status_of(seen).starts_with(" kiri "), "{}", status_of(seen));
    });
}

#[test]
fn a_click_jump_gives_the_focus_back_to_the_document_and_goes_into_the_history() {
    on_both(|| {
        let source = many(30);
        let at = click(8, 6);
        // With the focus in the bar, the click jumps and the status line is
        // the document's again; Backspace goes back to the top.
        let run = ran(&source, 120, Visibility::Auto, &[b"t", &at, b"\x7f", b"q"]);
        let screens = run.seen();
        let jumped = &screens[screens.len() - 2];
        assert_eq!(first_line(jumped), "H05", "{}", jumped.text());
        assert!(
            status_of(jumped).starts_with(" kiri "),
            "{}",
            status_of(jumped)
        );
        let back = screens.last().unwrap();
        assert_eq!(first_line(back), "H00", "{}", back.text());
    });
}

#[test]
fn a_click_on_a_row_of_a_scrolled_bar_jumps_to_the_entry_on_that_row() {
    on_both(|| {
        let source = many(30);
        let (wheel, at) = (wheel_down(5, 3), click(8, 2));
        let run = ran(&source, 120, Visibility::Auto, &[&wheel, &at, b"q"]);
        // Scrolled by one notch, row 2 is H04.
        let seen = run.seen().last().unwrap();
        assert_eq!(first_line(seen), "H04", "{}", seen.text());
    });
}

#[test]
fn a_click_on_the_fold_glyph_folds_and_opens_the_entry_and_keeps_the_focus() {
    on_both(|| {
        // Alpha, its children Beta and Gamma; the glyph is in column 2.
        let at = click(2, 1);
        let folded = ran(SOURCE, 120, Visibility::Auto, &[&at, b"q"]);
        let seen = folded.seen().last().unwrap();
        assert!(!bar_of(seen).contains("Beta"), "{}", bar_of(seen));
        assert!(bar_of(seen).contains('▸'), "{}", bar_of(seen));
        // The document did not move, and a second click opens it again.
        assert!(status_of(seen).starts_with(" kiri "), "{}", status_of(seen));
        let again = ran(SOURCE, 120, Visibility::Auto, &[&at, &at, b"q"]);
        assert!(bar_of(again.seen().last().unwrap()).contains("Beta"));
        // With the focus in the bar it stays there.
        let focused = ran(SOURCE, 120, Visibility::Auto, &[b"t", &at, b"q"]);
        let seen = focused.seen().last().unwrap();
        assert!(!bar_of(seen).contains("Beta"), "{}", bar_of(seen));
        assert!(
            status_of(seen).starts_with(" contents "),
            "{}",
            status_of(seen)
        );
    });
}

#[test]
fn a_click_on_an_empty_row_or_on_the_border_does_nothing() {
    on_both(|| {
        let top = ran(SOURCE, 120, Visibility::Auto, &[b"q"]);
        // SOURCE has three entries: row 8 is empty, and column 30 is the
        // border, on the row of an entry too.
        for (column, row) in [(8, 8), (30, 2), (30, 8)] {
            let at = click(column, row);
            let run = ran(SOURCE, 120, Visibility::Auto, &[&at, b"q"]);
            assert_eq!(
                run.seen().last().unwrap().text(),
                top.seen()[0].text(),
                "{column},{row}"
            );
        }
    });
}
