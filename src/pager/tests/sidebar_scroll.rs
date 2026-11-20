//! Where the contents bar's list starts: it moves only as far as the
//! heading the reader is in needs, and not at all while the wheel has it.
//! The window is 120 × 12, so the bar has 11 rows; `many(30)` lists 30
//! headings, `H00` to `H29`.

use super::sidebar::{bar_of, click, document_of, edges, long_paragraph, many, ran};
use super::*;
use crate::session::sidebar::Visibility;

/// A notch of the wheel down at `column` and `row`, from 1.
fn wheel_down(column: usize, row: usize) -> Vec<u8> {
    format!("\x1b[<65;{column};{row}M").into_bytes()
}

/// The first entry of the bar on `seen`, as the number in its `H` name.
fn first_entry(seen: &Seen) -> usize {
    let bar = bar_of(seen);
    let row = bar.lines().next().unwrap();
    let at = row.find('H').unwrap_or_else(|| panic!("an entry in {row}"));
    row[at + 1..at + 3].trim().parse().unwrap()
}

/// A notch of the wheel up at `column` and `row`, from 1.
fn wheel_up(column: usize, row: usize) -> Vec<u8> {
    format!("\x1b[<64;{column};{row}M").into_bytes()
}

/// The row of the bar, from 1, with the entry the reader is in, if the bar
/// shows it.
fn mark_row(seen: &Seen) -> Option<usize> {
    edges(seen).first().copied()
}

/// The screens of a run of `source` that starts at 120 × 12 and is resized
/// to each of `sizes` after the reads, `keys` being read.
fn ran_resized(source: &str, sizes: &[(u16, u16)], keys: &[&[u8]]) -> Fake {
    let document = crate::typeset::parse::parse(source);
    let mut terminal = Fake::new(120, 12, keys);
    terminal.sizes = sizes
        .iter()
        .map(|&(columns, rows)| window(columns, rows))
        .collect();
    let mut inputs = Inputs::new(screen_caps(120));
    let options = Options {
        sidebar: Visibility::Auto,
        ..Options::new(SearchCase::Smart)
    };
    run_document(&document, &mut inputs, options, &mut terminal);
    terminal
}

#[test]
fn a_scroll_of_the_document_moves_the_bar_only_when_the_mark_would_leave_it() {
    on_both(|| {
        let source = many(30);
        let mut keys: Vec<&[u8]> = vec![&b"j"[..]; 70];
        keys.push(b"q");
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let mut started = false;
        for seen in run.seen() {
            let row = mark_row(seen).unwrap_or_else(|| panic!("a mark in\n{}", bar_of(seen)));
            // The bar stays at the top while the mark goes down it; then
            // the mark stays on the last row and the list moves under it.
            match first_entry(seen) {
                0 => assert!(!started && row <= 11, "row {row}\n{}", bar_of(seen)),
                _ => {
                    started = true;
                    assert_eq!(row, 11, "{}", bar_of(seen));
                }
            }
        }
        assert!(started, "the bar moved at all");
    });
}

#[test]
fn going_back_up_the_document_leaves_the_bar_until_the_mark_reaches_its_first_row() {
    on_both(|| {
        let source = many(30);
        let mut keys: Vec<&[u8]> = vec![&b"j"[..]; 70];
        keys.extend(vec![&b"k"[..]; 70]);
        keys.push(b"q");
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let screens = &run.seen()[70..];
        let down = first_entry(&screens[0]);
        assert!(down > 0);
        let mut middle = false;
        let mut least = down;
        for seen in screens {
            let (first, row) = (first_entry(seen), mark_row(seen).unwrap());
            // The list moves up only with the mark on its first row.
            if first < least {
                assert_eq!(row, 1, "{}", bar_of(seen));
                least = first;
            } else if first == down && row > 1 && row < 11 {
                middle = true;
            }
        }
        assert!(middle, "the mark went up the bar with the list in place");
        assert!(least < down, "the list followed the mark up");
    });
}

#[test]
fn a_click_on_a_heading_in_view_leaves_the_bar_where_it_is() {
    on_both(|| {
        let source = many(30);
        // At the top: the click on row 8 (H07) does not centre H07.
        let at = click(5, 8);
        let run = ran(&source, 120, Visibility::Auto, &[&at, b"q"]);
        let seen = &run.seen()[1];
        assert_eq!(first_entry(seen), 0, "{}", bar_of(seen));
        assert_eq!(mark_row(seen), Some(8), "{}", bar_of(seen));
        // At the end, with the last 11 listed: the click on row 5 (H23).
        let run = ran(&source, 120, Visibility::Auto, &[b"G", &click(5, 5), b"q"]);
        let seen = run.seen().last().unwrap();
        assert_eq!(first_entry(seen), 19, "{}", bar_of(seen));
        assert_eq!(mark_row(seen), Some(5), "{}", bar_of(seen));
    });
}

#[test]
fn enter_on_a_heading_in_view_leaves_the_bar_where_it_is() {
    on_both(|| {
        let source = many(30);
        // `t` and nine `j` select H09 and the bar centres it (H04 on top);
        // a notch of the wheel moves the list to H07, and Enter goes to H09,
        // which is on its third row.
        let wheel = wheel_down(5, 3);
        let mut keys: Vec<&[u8]> = vec![b"t"];
        keys.extend(vec![&b"j"[..]; 9]);
        keys.extend([&wheel[..], b"\r", b"q"]);
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let seen = run.seen().last().unwrap();
        assert_eq!(first_entry(seen), 7, "{}", bar_of(seen));
        assert_eq!(mark_row(seen), Some(3), "{}", bar_of(seen));
    });
}

#[test]
fn a_jump_to_a_heading_below_the_bar_brings_it_to_the_last_row_and_one_above_to_the_first() {
    on_both(|| {
        let source = many(30);
        // Backspace after a click returns to the place before it, which has
        // the bar's heading above or below the rows it shows.
        let at = click(5, 11);
        let run = ran(&source, 120, Visibility::Auto, &[&at, b"G", b"\x7f", b"q"]);
        let screens = run.seen();
        // The click on H10 leaves the bar at the top; `G` brings H29 to the
        // last row, the list at 19; Backspace goes back to before the click,
        // H00, above the rows: it takes the first row.
        assert_eq!(first_entry(&screens[1]), 0, "{}", bar_of(&screens[1]));
        assert_eq!(first_entry(&screens[2]), 19, "{}", bar_of(&screens[2]));
        let last = screens.last().unwrap();
        assert_eq!(
            (first_entry(last), mark_row(last)),
            (0, Some(1)),
            "{}",
            bar_of(last)
        );
    });
}

#[test]
fn a_jump_to_a_heading_below_the_bar_brings_it_to_the_last_row_by_the_least() {
    on_both(|| {
        let source = many(30);
        // Down 40 lines puts the mark on a middle row of a bar at the top;
        // `G` takes H29 to the last row, the list moving the least.
        let mut keys: Vec<&[u8]> = vec![&b"j"[..]; 8];
        keys.extend([&b"G"[..], b"q"]);
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let last = run.seen().last().unwrap();
        assert_eq!(
            (first_entry(last), mark_row(last)),
            (19, Some(11)),
            "{}",
            bar_of(last)
        );
    });
}

#[test]
fn the_wheel_leaves_the_mark_out_of_view_and_the_next_scroll_of_the_document_brings_it_back() {
    on_both(|| {
        let source = many(30);
        let wheel = wheel_down(5, 3);
        // 40 lines down: the mark is on H08 or so, in the bar at the top.
        let mut keys: Vec<&[u8]> = vec![&b"j"[..]; 40];
        keys.extend(vec![&wheel[..]; 6]);
        keys.extend([&b"j"[..], b"q"]);
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let screens = run.seen();
        let n = screens.len();
        // Six notches down: 18 rows from the top; the mark is above the rows
        // the bar shows.
        let free = &screens[n - 2];
        assert_eq!(first_entry(free), 18, "{}", bar_of(free));
        assert_eq!(mark_row(free), None, "{}", bar_of(free));
        // The scroll of the document brings it to the first row, the least
        // that shows it.
        let back = &screens[n - 1];
        assert_eq!(mark_row(back), Some(1), "{}", bar_of(back));
        assert!(first_entry(back) < 18, "{}", bar_of(back));
    });
}

#[test]
fn a_resize_that_changes_the_rows_keeps_the_first_row_and_the_mark_in_view() {
    on_both(|| {
        let source = many(30);
        let mut keys: Vec<&[u8]> = vec![&b"j"[..]; 70];
        keys.push(b"q");
        // Before the resize: the mark on the last of the bar's 11 rows.
        let before = ran(&source, 120, Visibility::Auto, &keys);
        let seen = before.seen().last().unwrap();
        let (first, row) = (first_entry(seen), mark_row(seen).unwrap());
        assert_eq!(row, 11);
        let resized = |rows: u16| {
            let mut sizes = vec![(120, 12); 69];
            sizes.push((120, rows));
            let run = ran_resized(&source, &sizes, &keys);
            let after = run
                .seen()
                .iter()
                .find(|seen| seen.rows.len() == usize::from(rows));
            after.cloned().expect("a screen of the new size")
        };
        // Shorter, 7 rows: the mark is below them, and the list moves down
        // by the rows that are gone and no more.
        let seen = resized(8);
        assert_eq!(mark_row(&seen), Some(7), "{}", bar_of(&seen));
        assert_eq!(first_entry(&seen), first + 4, "{}", bar_of(&seen));
        // Taller, 15 rows: the list stays, and the mark with it.
        let seen = resized(16);
        assert_eq!(first_entry(&seen), first, "{}", bar_of(&seen));
        assert_eq!(mark_row(&seen), Some(11), "{}", bar_of(&seen));
    });
}

#[test]
fn a_jump_that_leaves_the_view_where_it_is_brings_the_mark_back_into_the_bar() {
    on_both(|| {
        // The link goes to H29, which is on the screen at the end already.
        let source = format!("{}[back](#h29)\n", many(30));
        let up = wheel_up(5, 3);
        let mut keys: Vec<&[u8]> = vec![b"G"];
        keys.extend(vec![&up[..]; 4]);
        keys.extend([&b"\t"[..], b"\r", b"q"]);
        let run = ran(&source, 120, Visibility::Auto, &keys);
        let screens = run.seen();
        // Before the jump the wheel left the mark out of view, at H07.
        let free = &screens[screens.len() - 3];
        assert_eq!(first_entry(free), 7, "{}", bar_of(free));
        assert_eq!(mark_row(free), None, "{}", bar_of(free));
        // After it the list moved the least that shows H29: to the last row.
        let last = screens.last().unwrap();
        assert_eq!(mark_row(last), Some(11), "{}", bar_of(last));
        assert_eq!(first_entry(last), 19, "{}", bar_of(last));
    });
}

#[test]
fn a_new_layout_that_moves_the_top_line_carries_the_bars_own_scroll_over() {
    on_both(|| {
        // 17 words of 5 columns are 84: one row beside a bar of 30 in 120
        // columns, two in 110, so the line at the top of the view is another
        // one after the resize.
        let section = |n: usize| format!("# H{n:02}\n\n{}\n\n", long_paragraph(17));
        let source: String = (0..30).map(section).collect();
        let wheel = wheel_down(5, 3);
        let mut keys: Vec<&[u8]> = vec![&b"j"[..]; 12];
        keys.extend([&wheel[..], &wheel[..], b"x", b"q"]);
        let mut sizes = vec![(120, 12); 14];
        sizes.push((110, 12));
        let run = ran_resized(&source, &sizes, &keys);
        let screens = run.seen();
        // The wheel put the list at H06; the resize leaves it there, with the
        // heading the reader is in out of view.
        let wheeled = &screens[14];
        assert_eq!(first_entry(wheeled), 6, "{}", bar_of(wheeled));
        let after = &screens[15];
        assert_ne!(document_of(after), document_of(wheeled));
        assert_eq!(first_entry(after), 6, "{}", bar_of(after));
    });
}
