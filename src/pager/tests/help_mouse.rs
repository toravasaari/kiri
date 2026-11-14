//! The mouse in the help: the wheel, the bar and the clicks. The window is
//! 60 × 24: the frame is 50 cells wide from column 6 to 55, the bar is in
//! column 54, and the lines of the help show on rows 2 to 22.

use super::*;
use crate::session::panels::{Shape, help};
use crate::typeset::theme::Theme;

const BAR: u16 = 54;
const WHEEL_DOWN: &[u8] = b"\x1b[<65;10;10M";
const WHEEL_DOWN_OUTSIDE: &[u8] = b"\x1b[<65;2;10M";

fn click(column: u16, row: u16) -> Vec<u8> {
    format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m").into_bytes()
}

fn screens_of(document: &Document, keys: &[&[u8]]) -> Vec<Vec<String>> {
    replay(&shown(document, 60, 24, keys))
}

/// The frame's cells of `row` of `screen`, without styles. The replay
/// of a screen writes a row's text one piece after the other, whatever
/// column it is for, and the frame is written last.
fn frame_row(screen: &[String], row: u16) -> String {
    let row: Vec<char> = unstyled(screen_row(screen, row)).chars().collect();
    row[row.len().saturating_sub(50)..].iter().collect()
}

/// What the document wrote on `row` of `screen`, before the frame.
fn under_frame(screen: &[String], row: u16) -> String {
    let row: Vec<char> = unstyled(screen_row(screen, row)).chars().collect();
    row[..row.len().saturating_sub(50)].iter().collect()
}

fn help_open(screen: &[String]) -> bool {
    screen_text(screen).contains(" Help ")
}

fn help_lines() -> usize {
    help::lines(help::fit(), &help::Look::new(&Theme::default())).len()
}

#[test]
fn the_bar_is_where_the_tests_below_click() {
    let screens = screens_of(&numbers(30), &[b"?"]);
    let row = unstyled(screen_row(&screens[1], 2));
    assert!(row.ends_with("█│"), "{row}");
    assert!(help_lines() > 21);
}

#[test]
fn the_wheel_scrolls_the_help_inside_the_frame_and_outside_it() {
    let screens = screens_of(&numbers(30), &[b"?", WHEEL_DOWN, WHEEL_DOWN_OUTSIDE]);
    let (open, inside, outside) = (&screens[1], &screens[2], &screens[3]);
    // Three lines at a time: the row that was fourth is the first now.
    assert_eq!(frame_row(inside, 2)[..40], frame_row(open, 5)[..40]);
    assert_eq!(frame_row(outside, 2)[..40], frame_row(open, 8)[..40]);
    // The document under the help stays where it is.
    for row in 1..=12 {
        assert_eq!(under_frame(inside, row), under_frame(open, row));
        assert_eq!(under_frame(outside, row), under_frame(open, row));
    }
}

#[test]
fn a_click_on_the_track_below_the_bar_pages_down() {
    let paged = screens_of(&numbers(30), &[b"?", &click(BAR, 20)]);
    let keyed = screens_of(&numbers(30), &[b"?", b" "]);
    assert_eq!(paged.len(), 3, "the help moved: {paged:#?}");
    assert_eq!(paged[2], keyed[2]);
}

#[test]
fn a_click_on_the_track_above_the_bar_pages_up() {
    let paged = screens_of(&numbers(30), &[b"?", b"G", &click(BAR, 3)]);
    let keyed = screens_of(&numbers(30), &[b"?", b"G", b"b"]);
    assert_eq!(paged.len(), 4, "the help moved: {paged:#?}");
    assert_eq!(paged[3], keyed[3]);
}

#[test]
fn dragging_the_bar_moves_the_help_to_the_line_it_stands_for() {
    let press: &[u8] = b"\x1b[<0;54;2M";
    let drag: &[u8] = b"\x1b[<32;54;12M";
    let release: &[u8] = b"\x1b[<0;54;12m";
    let dragged = screens_of(&numbers(30), &[b"?", press, drag, release]);
    // Ten rows down is twenty halves.
    let shape = Shape::new((60, 23), help::fit(), help_lines());
    let line = shape.line_at(20);
    assert!(line > 0);
    let mut keys: Vec<&[u8]> = vec![b"?"];
    keys.extend(std::iter::repeat_n(&b"j"[..], line));
    let keyed = screens_of(&numbers(30), &keys);
    assert_eq!(dragged.last(), keyed.last());
    // Let go, the bar stays: more motion does nothing.
    let after: &[u8] = b"\x1b[<32;54;20M";
    let more = screens_of(&numbers(30), &[b"?", press, drag, release, after]);
    assert_eq!(more.last(), dragged.last());
}

#[test]
fn a_click_inside_the_frame_elsewhere_does_nothing() {
    for (column, row) in [(20, 10), (7, 2), (BAR - 1, 10), (30, 1), (30, 23)] {
        let screens = screens_of(&numbers(30), &[b"?", &click(column, row)]);
        assert!(help_open(screens.last().unwrap()), "{column},{row}");
        assert_eq!(screens.last(), Some(&screens[1]), "{column},{row}");
    }
}

#[test]
fn a_click_outside_the_frame_closes_the_help_and_follows_no_link() {
    let filler: String = (1..=30).map(|n| format!("Line {n}\n\n")).collect();
    let document = crate::typeset::parse::parse(&format!("[jump](#end)\n\n{filler}# end\n"));
    // On the link, on the first row, left of the frame.
    for at in [(4, 1), (2, 10), (58, 10)] {
        let screens = screens_of(&document, &[b"?", &click(at.0, at.1)]);
        let last = screens.last().unwrap();
        assert!(!help_open(last), "{at:?}: {last:#?}");
        assert!(unstyled(&last[0]).contains("jump"), "{at:?}: {last:#?}");
        assert!(!screen_text(last).contains("# end"), "{at:?}: {last:#?}");
    }
}
