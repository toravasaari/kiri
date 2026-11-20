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

/// The left button pressed at `column` and `row`, from 1.
fn press(column: usize, row: usize) -> Vec<u8> {
    format!("\x1b[<0;{column};{row}M").into_bytes()
}

/// The pointer moved to `column` and `row` with the left button held.
fn drag(column: usize, row: usize) -> Vec<u8> {
    format!("\x1b[<32;{column};{row}M").into_bytes()
}

/// The left button let go at `column` and `row`.
fn release(column: usize, row: usize) -> Vec<u8> {
    format!("\x1b[<0;{column};{row}m").into_bytes()
}

/// The width of the bar on `seen`: the column of its border, from 1.
fn bar_width(seen: &Seen) -> usize {
    let row = &seen.rows[0];
    row.cells
        .iter()
        .position(|cell| cell.text == "▕")
        .map_or(0, |at| at + 1)
}

/// The bar's width after each of `keys`, in a window of `columns` with the
/// bar `visibility` asks for.
fn widths(columns: u16, visibility: Visibility, keys: &[&[u8]]) -> Vec<usize> {
    let mut all: Vec<&[u8]> = keys.to_vec();
    all.push(b"q");
    let run = ran(&many(30), columns, visibility, &all);
    run.seen().iter().map(bar_width).collect()
}

#[test]
fn dragging_the_border_changes_the_width_of_the_bar() {
    on_both(|| {
        let (down, to, up) = (press(30, 3), drag(45, 3), release(45, 3));
        let widths = widths(120, Visibility::Auto, &[&down, &to, &up]);
        assert_eq!(widths.first(), Some(&30));
        assert_eq!(widths.last(), Some(&45), "{widths:?}");
        // Back to the left, narrower than the default.
        let back = drag(20, 3);
        let widths = self::widths(120, Visibility::Auto, &[&down, &back, &release(20, 3)]);
        assert_eq!(widths.last(), Some(&20), "{widths:?}");
    });
}

#[test]
fn the_width_is_at_least_16_at_most_half_the_window_and_leaves_the_document_40_columns() {
    on_both(|| {
        let down = press(30, 3);
        let at = |column: usize, window: u16, visibility| {
            let (to, up) = (drag(column, 3), release(column, 3));
            // The border is at the bar's default width: a third of 70, else 30.
            let down = if window == 70 {
                press(23, 3)
            } else {
                down.clone()
            };
            let widths = self::widths(window, visibility, &[&down, &to, &up]);
            *widths.last().unwrap()
        };
        assert_eq!(at(3, 120, Visibility::Auto), 16, "at least 16");
        assert_eq!(at(110, 120, Visibility::Auto), 60, "at most half of 120");
        // A window of 70 opened by hand: half is 35, and 40 columns for the
        // document leave 30.
        assert_eq!(at(60, 70, Visibility::Open), 30);
        // A window of 90: half is 45 and 40 columns leave 50.
        assert_eq!(at(80, 90, Visibility::Open), 45);
    });
}

#[test]
fn a_press_off_the_border_starts_no_drag_and_a_release_ends_one() {
    on_both(|| {
        let (to, up) = (drag(45, 3), release(45, 3));
        // On an entry, and in the document: the motion changes nothing.
        for from in [press(10, 3), press(60, 3)] {
            let widths = widths(120, Visibility::Auto, &[&from, &to, &up]);
            assert!(widths.iter().all(|&width| width == 30), "{widths:?}");
        }
        // After the release the motion is not a drag any more.
        let (down, first, up) = (press(30, 3), drag(40, 3), release(40, 3));
        let more = drag(50, 3);
        let widths = widths(120, Visibility::Auto, &[&down, &first, &up, &more]);
        assert_eq!(widths.last(), Some(&40), "{widths:?}");
    });
}

#[test]
fn a_drag_lays_the_document_out_again_in_what_is_left_and_keeps_the_place() {
    on_both(|| {
        // 19 words of 5 columns are 94 columns of text: one row in the
        // 110 columns beside a bar of 30, two in the 80 beside one of 60.
        let source: String = std::iter::once("# Top\n\n## Sub\n\n".to_owned())
            .chain((1..=40).map(|n| format!("Paragraph {n}.\n\n")))
            .chain(std::iter::once(format!(
                "{}\n",
                super::sidebar::long_paragraph(19)
            )))
            .collect();
        let rows_of_paragraph = |seen: &Seen| {
            (seen.rows.iter())
                .filter(|row| row.find("w0").is_some())
                .count()
        };
        let (down, to, up) = (press(30, 3), drag(60, 3), release(60, 3));
        let keys: &[&[u8]] = &[b"G", &down, &to, &up, b"q"];
        let run = ran(&source, 140, Visibility::Auto, keys);
        let screens = run.seen();
        let widest = (screens.iter())
            .position(|seen| bar_width(seen) == 60)
            .expect("a screen with the wider bar");
        let (before, after) = (&screens[widest - 1], &screens[widest]);
        assert_eq!(rows_of_paragraph(before), 1, "{}", before.text());
        assert_eq!(rows_of_paragraph(after), 2, "{}", after.text());
        assert_eq!(bar_width(after), 60);
        // The end of the document is still on the screen.
        assert!(after.text().contains("w019"), "{}", after.text());
    });
}

const PAGE_DOWN: &[u8] = b"\x1b[6~";
const PAGE_UP: &[u8] = b"\x1b[5~";
const HOME: &[u8] = b"\x1b[H";
const END: &[u8] = b"\x1b[F";

#[test]
fn page_down_and_page_up_move_the_selection_by_the_rows_of_the_bar() {
    on_both(|| {
        let source = many(30);
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        // The bar has 11 rows: a page down from H00 selects H11, and the
        // document stays where it is until Enter jumps.
        let run = ran(&source, 120, Visibility::Auto, &[b"t", PAGE_DOWN, b"q"]);
        let seen = run.seen().last().unwrap();
        assert_eq!(document_of(seen), document_of(&top.seen()[0]));
        assert!(bar_of(seen).contains("H11"), "{}", bar_of(seen));
        let run = ran(
            &source,
            120,
            Visibility::Auto,
            &[b"t", PAGE_DOWN, b"\r", b"q"],
        );
        assert_eq!(first_line(run.seen().last().unwrap()), "H11");
        // Two pages down and one up.
        let keys: &[&[u8]] = &[b"t", PAGE_DOWN, PAGE_DOWN, PAGE_UP, b"\r", b"q"];
        let run = ran(&source, 120, Visibility::Auto, keys);
        assert_eq!(first_line(run.seen().last().unwrap()), "H11");
        // Past the end it stops at the last entry, and past the start at the first.
        let keys: &[&[u8]] = &[b"t", PAGE_DOWN, PAGE_DOWN, PAGE_DOWN, b"\r", b"q"];
        let run = ran(&source, 120, Visibility::Auto, keys);
        // The end of the document keeps H29 from the top line.
        assert!(document_of(run.seen().last().unwrap()).contains("H29"));
        let keys: &[&[u8]] = &[b"t", PAGE_UP, b"\r", b"q"];
        let run = ran(&source, 120, Visibility::Auto, keys);
        assert_eq!(first_line(run.seen().last().unwrap()), "H00");
    });
}

#[test]
fn home_and_end_select_the_first_and_last_entry_and_the_document_stays() {
    on_both(|| {
        let source = many(30);
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        let run = ran(&source, 120, Visibility::Auto, &[b"t", END, b"q"]);
        let seen = run.seen().last().unwrap();
        assert_eq!(document_of(seen), document_of(&top.seen()[0]));
        assert!(bar_of(seen).contains("H29"), "{}", bar_of(seen));
        let run = ran(&source, 120, Visibility::Auto, &[b"t", END, HOME, b"q"]);
        let seen = run.seen().last().unwrap();
        assert_eq!(document_of(seen), document_of(&top.seen()[0]));
        assert!(bar_of(seen).contains("H00"), "{}", bar_of(seen));
    });
}

#[test]
fn the_page_keys_scroll_the_document_when_the_bar_has_no_focus() {
    on_both(|| {
        let source = many(30);
        let top = ran(&source, 120, Visibility::Auto, &[b"q"]);
        let run = ran(&source, 120, Visibility::Auto, &[PAGE_DOWN, b"q"]);
        let seen = run.seen().last().unwrap();
        assert_ne!(document_of(seen), document_of(&top.seen()[0]));
    });
}

#[test]
fn a_fast_drag_in_one_read_ends_at_the_last_motion_and_the_release_ends_it() {
    on_both(|| {
        // The press, two motions and the release come in one read, as they do
        // when the terminal queued them while a layout was under way.
        let batch = [press(30, 3), drag(40, 3), drag(50, 3), release(50, 3)].concat();
        // A motion alone in a later read must not drag a bar that was let go.
        let later = drag(60, 3);
        let widths = widths(120, Visibility::Auto, &[&batch, &later]);
        assert_eq!(widths.last(), Some(&50), "{widths:?}");
        assert!(widths.iter().all(|&width| width != 60), "{widths:?}");
        // One layout for the whole batch: no screen at the first motion's 40.
        assert!(widths.iter().all(|&width| width != 40), "{widths:?}");
    });
}
