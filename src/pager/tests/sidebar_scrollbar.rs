//! The contents bar's scroll bar: a column next to the border, drawn when
//! the list is longer than the rows, that answers a click on its track and
//! a drag of its thumb. The window is 120 × 12: the bar is 30 columns wide
//! with 11 rows, the border is in column 30 and the scroll bar in 29.

use super::sidebar::{bar_of, click, document_of, many, ran};
use super::sidebar_mouse::{bar_width, drag, press, release};
use super::*;
use crate::session::sidebar::Visibility;

/// The cell of the scroll bar's column in each row of the bar on `seen`.
fn cells(seen: &Seen) -> Vec<String> {
    (seen.rows.iter())
        .take(seen.rows.len() - 1)
        .map(|row| row.cells.get(28).map_or(String::new(), |c| c.text.clone()))
        .collect()
}

/// The first entry of the bar on `seen`, as the number in its `H` name.
fn first_entry(seen: &Seen) -> usize {
    let bar = bar_of(seen);
    let row = bar.lines().next().unwrap();
    let at = row.find('H').unwrap_or_else(|| panic!("an entry in {row}"));
    row[at + 1..at + 3].trim().parse().unwrap()
}

#[test]
fn a_list_longer_than_the_rows_has_a_thumb_next_to_the_border_and_a_shorter_one_none() {
    on_both(|| {
        // 30 entries in 11 rows: a thumb of 8 half rows from the top.
        let run = ran(&many(30), 120, Visibility::Auto, &[b"q"]);
        let seen = &run.seen()[0];
        let column = cells(seen);
        assert_eq!(column[0], "█", "{column:?}\n{}", bar_of(seen));
        assert_eq!(column[3], "█", "{column:?}");
        assert_eq!(column[10], " ", "{column:?}");
        assert_eq!(bar_width(seen), 30);
        // At the end of the list it is at the bottom.
        let run = ran(&many(30), 120, Visibility::Auto, &[b"G", b"q"]);
        let column = cells(run.seen().last().unwrap());
        assert_eq!(
            (column[0].as_str(), column[10].as_str()),
            (" ", "█"),
            "{column:?}"
        );
        // 11 entries fit the 11 rows: no thumb, and a row as it was.
        let run = ran(&many(11), 120, Visibility::Auto, &[b"q"]);
        let column = cells(&run.seen()[0]);
        assert!(column.iter().all(|cell| cell == " "), "{column:?}");
    });
}

#[test]
fn the_text_of_the_rows_is_a_column_narrower_while_the_bar_shows() {
    on_both(|| {
        let title = "x".repeat(40);
        let long = |count: usize| -> String {
            (0..count)
                .map(|n| format!("# {title}{n:02}\n\nline\n\n"))
                .collect()
        };
        // Where the cut is, in cells: the ellipsis.
        let at = |count: usize| {
            let run = ran(&long(count), 120, Visibility::Auto, &[b"q"]);
            let row = &run.seen()[0].rows[0];
            row.cells.iter().position(|cell| cell.text == "…").unwrap()
        };
        assert_eq!(at(30), at(5) - 1);
    });
}

#[test]
fn a_click_on_the_track_pages_the_list_towards_it_and_does_not_jump() {
    on_both(|| {
        let source = many(30);
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        // Below the thumb: a page down, 11 rows, the document stays.
        let below = click(29, 11);
        let run = ran(&source, 120, Visibility::Auto, &[&below, b"q"]);
        let seen = &run.seen()[1];
        assert_eq!(first_entry(seen), 11, "{}", bar_of(seen));
        assert_eq!(document_of(seen), document_of(&top.seen()[0]));
        // And back up from the end, above the thumb.
        let above = click(29, 1);
        let run = ran(&source, 120, Visibility::Auto, &[b"G", &above, b"q"]);
        let seen = run.seen().last().unwrap();
        assert_eq!(first_entry(seen), 8, "{}", bar_of(seen));
        // On the thumb itself, a click does nothing.
        let on = click(29, 2);
        let run = ran(&source, 120, Visibility::Auto, &[&on, b"q"]);
        assert_eq!(run.seen().len(), 1);
    });
}

#[test]
fn dragging_the_thumb_scrolls_the_list_wherever_the_pointer_goes_across() {
    on_both(|| {
        let source = many(30);
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        // Taken at its second row, the thumb follows the pointer's row, even
        // when the pointer leaves the column.
        let (down, to, more, up) = (press(29, 2), drag(29, 8), drag(60, 12), release(60, 12));
        let run = ran(
            &source,
            120,
            Visibility::Auto,
            &[&down, &to, &more, &up, b"q"],
        );
        let screens = run.seen();
        let middle = &screens[1];
        assert_eq!(first_entry(middle), 16, "{}", bar_of(middle));
        assert_eq!(first_entry(&screens[2]), 19, "{}", bar_of(&screens[2]));
        // The width and the document did not move, nor did the mark.
        assert!(screens.iter().all(|seen| bar_width(seen) == 30));
        assert_eq!(document_of(&screens[2]), document_of(&top.seen()[0]));
        // The release lets go: the next motion drags nothing.
        let after = drag(29, 1);
        let run = ran(
            &source,
            120,
            Visibility::Auto,
            &[&down, &to, &up, &after, b"q"],
        );
        let last = run.seen().last().unwrap();
        assert_eq!(first_entry(last), 16, "{}", bar_of(last));
    });
}

#[test]
fn the_thumb_and_the_border_are_dragged_each_on_its_own() {
    on_both(|| {
        let source = many(30);
        // The border dragged: the width changes and the list stays.
        let (down, to, up) = (press(30, 3), drag(45, 3), release(45, 3));
        let run = ran(&source, 120, Visibility::Auto, &[&down, &to, &up, b"q"]);
        let last = run.seen().last().unwrap();
        assert_eq!(bar_width(last), 45);
        assert_eq!(first_entry(last), 0);
        // Then the thumb, in the wider bar, where it is next to the border.
        let (hold, pull, let_go) = (press(44, 2), drag(44, 8), release(44, 8));
        let keys: Vec<&[u8]> = vec![&down, &to, &up, &hold, &pull, &let_go, b"q"];
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let last = run.seen().last().unwrap();
        assert_eq!(
            (bar_width(last), first_entry(last)),
            (45, 16),
            "{}",
            bar_of(last)
        );
        // The thumb dragged first, then the border: each takes its own.
        let (down2, to2, up2) = (press(30, 3), drag(40, 3), release(40, 3));
        let (hold, pull, let_go) = (press(29, 2), drag(29, 8), release(29, 8));
        let keys: Vec<&[u8]> = vec![&hold, &pull, &let_go, &down2, &to2, &up2, b"q"];
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let last = run.seen().last().unwrap();
        assert_eq!(
            (bar_width(last), first_entry(last)),
            (40, 16),
            "{}",
            bar_of(last)
        );
    });
}

#[test]
fn the_wheel_moves_the_thumb() {
    on_both(|| {
        let source = many(30);
        let wheel = format!("\x1b[<65;5;3M").into_bytes();
        let run = ran(&source, 120, Visibility::Auto, &[&wheel, b"q"]);
        let screens = run.seen();
        // Three rows down: the thumb has left the top row, whole.
        let (before, after) = (cells(&screens[0]), cells(&screens[1]));
        assert_eq!(before[0], "█", "{before:?}");
        assert_ne!(after[0], "█", "{after:?}");
        assert_ne!(before, after);
    });
}

#[test]
fn a_scroll_bar_scroll_is_as_free_as_the_wheel() {
    on_both(|| {
        let source = many(30);
        let page = click(29, 11);
        let run = ran(&source, 120, Visibility::Auto, &[&page, b"j", b"q"]);
        let screens = run.seen();
        // The page leaves the mark (H00) out of the bar; the next scroll of
        // the document brings it back by the least move: to the first row.
        assert!(!bar_of(&screens[1]).contains('▌'));
        assert_eq!(first_entry(&screens[2]), 0, "{}", bar_of(&screens[2]));
    });
}
