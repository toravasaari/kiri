//! The scroll bar's geometry: where the thumb is, what a press on the bar
//! takes hold of and where a drag of the thumb puts the list. The bar
//! leaves half a row free at its top and at its bottom, from three rows
//! on, so `2 * rows - 2` half rows are the thumb's.

use super::*;

/// Bars of 1 to 40 rows and lists from one row longer than the bar to
/// 200 rows, as `(listed, rows)`.
fn lists() -> impl Iterator<Item = (usize, usize)> {
    (1..=40).flat_map(|rows| (rows + 1..=200).map(move |listed| (listed, rows)))
}

#[test]
fn a_list_that_fits_the_rows_has_no_thumb() {
    assert_eq!(thumb(11, 11, 0), None);
    assert_eq!(thumb(5, 11, 0), None);
    assert_eq!(thumb(0, 11, 0), None);
    assert_eq!(thumb(30, 0, 0), None);
}

#[test]
fn the_thumb_of_a_bar_of_three_rows_or_more_starts_after_the_free_half_row() {
    // 30 entries in 11 rows: the 20 half rows between the free ones, a
    // thumb of 7 of them, from the second half row.
    assert_eq!(thumb(30, 11, 0), Some((1, 7)));
    // At the end it stops before the last half row, the 22nd.
    assert_eq!(thumb(30, 11, 19), Some((14, 7)));
    // The smallest bar with an inset: two half rows between the free ones.
    assert_eq!(thumb(10, 3, 0), Some((1, 2)));
    assert_eq!(thumb(10, 3, 7), Some((3, 2)));
}

#[test]
fn a_bar_of_fewer_than_three_rows_has_no_free_half_rows() {
    // The whole bar is the thumb's room, as in the help.
    assert_eq!(thumb(4, 2, 0), Some((0, 2)));
    assert_eq!(thumb(4, 2, 2), Some((2, 2)));
    assert_eq!(thumb(3, 1, 0), Some((0, 2)));
    assert_eq!(thumb(3, 1, 2), Some((0, 2)));
}

#[test]
fn the_thumb_is_never_smaller_than_two_half_rows() {
    assert_eq!(thumb(1000, 11, 0), Some((1, 2)));
    assert_eq!(thumb(1000, 11, 989), Some((19, 2)));
    for (listed, rows) in lists() {
        let (_, size) = thumb(listed, rows, 0).unwrap();
        assert!(size >= 2, "{listed} listed, {rows} rows: {size}");
    }
}

#[test]
fn the_thumb_stays_in_the_half_rows_between_the_free_ones_at_every_place() {
    for (listed, rows) in lists().filter(|&(_, rows)| rows >= INSET_MIN_ROWS) {
        for first in 0..=listed - rows {
            let (start, size) = thumb(listed, rows, first).unwrap();
            assert!(start >= 1, "{listed} listed, {rows} rows, from {first}");
            assert!(
                start + size < 2 * rows,
                "{listed} listed, {rows} rows, from {first}: {start} + {size}"
            );
        }
    }
}

#[test]
fn the_thumb_goes_from_the_first_place_to_the_last_and_does_not_move_back() {
    for (listed, rows) in lists().filter(|&(_, rows)| rows >= INSET_MIN_ROWS) {
        let (first, size) = thumb(listed, rows, 0).unwrap();
        assert_eq!(first, 1, "{listed} listed, {rows} rows");
        let (last, _) = thumb(listed, rows, listed - rows).unwrap();
        assert_eq!(last + size, 2 * rows - 1, "{listed} listed, {rows} rows");
        let mut before = 0;
        for at in 0..=listed - rows {
            let (start, _) = thumb(listed, rows, at).unwrap();
            assert!(start >= before, "{listed} listed, {rows} rows, from {at}");
            before = start;
        }
    }
}

#[test]
fn a_press_on_the_half_row_free_at_an_end_takes_what_is_next_to_it() {
    // The thumb at the top, from the second half row: a press on the first
    // row, whose upper half is free, takes the thumb.
    assert_eq!(press_thumb((30, 11, 0), 1), Some(Grabbed::Grip(0)));
    // At the end, a press on the last row, whose lower half is free, takes
    // it, 6 half rows from its top.
    assert_eq!(press_thumb((30, 11, 19), 11), Some(Grabbed::Grip(6)));
    // The track at the other end is a page.
    assert_eq!(
        press_thumb((30, 11, 0), 11),
        Some(Grabbed::Page { down: true })
    );
    assert_eq!(
        press_thumb((30, 11, 19), 1),
        Some(Grabbed::Page { down: false })
    );
}

#[test]
fn a_press_on_the_track_pages_towards_it_and_one_on_the_thumb_takes_hold() {
    // The thumb at 14..21 half rows, the rows 8 to 11.
    let at = |row| press_thumb((30, 11, 19), row);
    assert_eq!(at(2), Some(Grabbed::Page { down: false }));
    assert_eq!(at(7), Some(Grabbed::Page { down: false }));
    assert_eq!(at(8), Some(Grabbed::Grip(0)));
    assert_eq!(at(9), Some(Grabbed::Grip(2)));
    assert_eq!(at(10), Some(Grabbed::Grip(4)));
    let at = |row| press_thumb((30, 11, 0), row);
    assert_eq!(at(2), Some(Grabbed::Grip(1)));
    assert_eq!(at(4), Some(Grabbed::Grip(5)));
    assert_eq!(at(5), Some(Grabbed::Page { down: true }));
}

#[test]
fn a_press_outside_the_rows_or_on_a_list_that_fits_takes_nothing() {
    assert_eq!(press_thumb((30, 11, 0), 0), None);
    assert_eq!(press_thumb((30, 11, 0), 12), None);
    assert_eq!(press_thumb((11, 11, 0), 1), None);
}

#[test]
fn dragging_the_thumb_to_the_first_and_the_last_place_gives_the_first_and_the_last_listed_row() {
    for (listed, rows) in lists().filter(|&(_, rows)| rows >= 2) {
        assert_eq!(dragged_to((listed, rows), 0, 1), 0, "{listed}, {rows}");
        assert_eq!(
            dragged_to((listed, rows), 0, rows),
            listed - rows,
            "{listed} listed, {rows} rows"
        );
        // The pointer far below the bar goes no further.
        assert_eq!(
            dragged_to((listed, rows), 0, rows + 20),
            listed - rows,
            "{listed} listed, {rows} rows"
        );
    }
}

#[test]
fn the_thumb_taken_by_its_second_row_reaches_the_ends_with_the_pointer_in_the_first_and_last_rows()
{
    // The pointer in row 1 pulls the thumb above its start: the top.
    assert_eq!(dragged_to((30, 11), 3, 1), 0);
    // The grip is 6 half rows from the thumb's top and the pointer in the
    // last row, half row 21: the thumb's end is at the last place.
    assert_eq!(dragged_to((30, 11), 6, 11), 19);
}

#[test]
fn dragging_to_where_the_thumb_is_leaves_it_there_when_it_starts_on_a_whole_row() {
    // Taken where the press found it, in the row of its first half, and
    // dragged to that row: a round trip of `thumb` and `dragged_to`. A
    // thumb that starts in the lower half of a row is taken by the upper
    // half, a half row above it, and moves up to two half rows (a quarter
    // first motion.
    for (listed, rows) in lists().filter(|&(_, rows)| rows >= INSET_MIN_ROWS) {
        for first in 0..=listed - rows {
            let (start, size) = thumb(listed, rows, first).unwrap();
            let row = start / 2 + 1;
            let Some(Grabbed::Grip(grip)) = press_thumb((listed, rows, first), row) else {
                panic!("{listed} listed, {rows} rows, from {first}: row {row}");
            };
            let dragged = dragged_to((listed, rows), grip, row);
            let (after, after_size) = thumb(listed, rows, dragged).unwrap();
            assert_eq!(size, after_size);
            let within = if start % 2 == 0 { 0 } else { 2 };
            assert!(
                start.abs_diff(after) <= within,
                "{listed} listed, {rows} rows, from {first}: {start} -> {after} \
                 (dragged to {dragged})"
            );
        }
    }
}

#[test]
fn dragging_the_thumb_down_never_moves_the_list_up() {
    for (listed, rows) in lists().filter(|&(listed, rows)| rows >= 2 && listed < 80) {
        let mut before = 0;
        for row in 1..=rows {
            let at = dragged_to((listed, rows), 0, row);
            assert!(at >= before, "{listed} listed, {rows} rows, row {row}");
            before = at;
        }
    }
}
