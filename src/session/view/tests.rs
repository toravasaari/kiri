use super::*;

/// A view of `count` lines one row high in a window of `rows` rows.
fn plain(count: usize, rows: usize) -> View {
    View::new(vec![1; count], rows)
}

#[test]
fn the_view_is_at_the_end_on_the_last_page_of_a_document_longer_than_the_window() {
    let mut view = plain(30, 10);
    assert!(!view.at_end());
    view.scroll(Motion::Down(5));
    assert!(!view.at_end());
    view.scroll(Motion::End);
    assert!(view.at_end());
    // A document that fits is at its top, not scrolled to its end.
    assert!(!plain(5, 10).at_end());
    assert!(!scrolled(plain(5, 10), &[Motion::End]).at_end());
}

fn scrolled(mut view: View, motions: &[Motion]) -> View {
    for &motion in motions {
        view.scroll(motion);
    }
    view
}

#[test]
fn shows_the_lines_that_fit_from_the_top() {
    assert_eq!(plain(10, 4).visible(), 0..4);
    assert_eq!(plain(3, 4).visible(), 0..3);
}

#[test]
fn scrolls_by_lines_no_further_than_the_last_page() {
    let view = scrolled(plain(10, 4), &[Motion::Down(1)]);
    assert_eq!(view.visible(), 1..5);
    let view = scrolled(view, &[Motion::Down(100)]);
    assert_eq!(view.visible(), 8..10);
    let view = scrolled(view, &[Motion::Up(2)]);
    assert_eq!(view.visible(), 6..10);
    let view = scrolled(view, &[Motion::Up(100)]);
    assert_eq!(view.visible(), 0..4);
}

#[test]
fn a_document_that_fits_does_not_scroll() {
    let view = scrolled(plain(3, 4), &[Motion::Down(1), Motion::End]);
    assert_eq!(view.top(), 0);
    // Not even to make room for the margin at the end.
    let view = scrolled(plain(4, 4), &[Motion::Down(1), Motion::End]);
    assert_eq!(view.top(), 0);
}

#[test]
fn the_last_page_ends_in_two_empty_rows() {
    let view = scrolled(plain(10, 6), &[Motion::End]);
    assert_eq!(view.visible(), 6..10);
    assert_eq!(view.line_at(3), Some(9));
    assert_eq!(view.line_at(4), None);
    assert_eq!(view.position().percent, 100);
}

#[test]
fn a_last_line_too_tall_for_the_margin_gets_less_of_it() {
    let view = scrolled(View::new(vec![1, 1, 1, 1, 1, 3], 4), &[Motion::End]);
    assert_eq!(view.visible(), 5..6);
    assert_eq!(view.line_at(2), Some(5));
    assert_eq!(view.line_at(3), None);
}

#[test]
fn pages_move_by_the_lines_shown() {
    let view = scrolled(plain(10, 4), &[Motion::PageDown]);
    assert_eq!(view.visible(), 4..8);
    let view = scrolled(view, &[Motion::PageDown]);
    assert_eq!(view.visible(), 8..10);
    let view = scrolled(view, &[Motion::PageUp]);
    assert_eq!(view.visible(), 4..8);
    let view = scrolled(view, &[Motion::PageUp]);
    assert_eq!(view.visible(), 0..4);
}

#[test]
fn half_pages_move_by_half_the_window() {
    let view = scrolled(plain(20, 6), &[Motion::HalfPageDown]);
    assert_eq!(view.top(), 3);
    let view = scrolled(view, &[Motion::HalfPageUp]);
    assert_eq!(view.top(), 0);
    // A window of one row still moves.
    assert_eq!(scrolled(plain(5, 1), &[Motion::HalfPageDown]).top(), 1);
}

#[test]
fn start_and_end_jump_to_the_first_and_the_last_page() {
    let view = scrolled(plain(10, 4), &[Motion::End]);
    assert_eq!(view.visible(), 8..10);
    let view = scrolled(view, &[Motion::Start]);
    assert_eq!(view.visible(), 0..4);
}

#[test]
fn a_tall_line_scrolls_away_whole() {
    // A heading two rows high, then plain lines.
    let view = View::new(vec![2, 1, 1, 1, 1, 1], 3);
    assert_eq!(view.visible(), 0..2);
    let view = scrolled(view, &[Motion::Down(1)]);
    assert_eq!(view.visible(), 1..4);
    let view = scrolled(view, &[Motion::Up(1)]);
    assert_eq!(view.visible(), 0..2);
}

#[test]
fn a_tall_line_that_does_not_fit_at_the_bottom_is_left_out() {
    let view = View::new(vec![1, 1, 2, 1, 1], 3);
    assert_eq!(view.visible(), 0..2);
    let view = scrolled(view, &[Motion::Down(1)]);
    assert_eq!(view.visible(), 1..3);
}

#[test]
fn the_last_page_shows_the_last_line_whole() {
    let view = scrolled(View::new(vec![1, 1, 1, 1, 2], 4), &[Motion::End]);
    assert_eq!(view.visible(), 4..5);
    let view = scrolled(View::new(vec![1, 1, 1, 3, 1], 3), &[Motion::End]);
    assert_eq!(view.visible(), 4..5);
}

#[test]
fn paging_past_a_tall_line_moves_at_least_one_line() {
    // A heading taller than the window cannot be shown, but paging
    // still gets past it.
    let view = View::new(vec![1, 4, 1, 1, 1], 3);
    assert_eq!(view.visible(), 0..1);
    let view = scrolled(view, &[Motion::PageDown]);
    assert_eq!(view.top(), 1);
    let view = scrolled(view, &[Motion::PageDown]);
    assert_eq!(view.top(), 2);
    let view = scrolled(view, &[Motion::PageUp]);
    assert_eq!(view.top(), 1);
}

#[test]
fn a_page_up_shows_the_lines_above_whole() {
    // From line 4, a page up of three rows fits the heading of two
    // rows and one line, not the line above them.
    let view = View::new(vec![1, 1, 2, 1, 1, 1, 1], 3);
    let view = scrolled(view, &[Motion::Down(4), Motion::PageUp]);
    assert_eq!(view.top(), 2);
}

#[test]
fn an_empty_document_stays_at_the_start() {
    let mut view = View::new(Vec::new(), 4);
    for motion in [Motion::Down(1), Motion::PageDown, Motion::End] {
        view.scroll(motion);
        assert_eq!(view.visible(), 0..0, "{motion:?}");
    }
    view.restore(&Anchor::End, &laid("", false, 20).1, false);
    assert_eq!(view.top(), 0);
}

#[test]
fn the_position_is_the_share_of_rows_down_to_the_bottom_of_the_view() {
    let position = |percent| Position { percent };
    assert_eq!(plain(10, 4).position(), position(40));
    let view = scrolled(plain(10, 4), &[Motion::End]);
    assert_eq!(view.position(), position(100));
    assert_eq!(plain(3, 4).position(), position(100));
    assert_eq!(plain(0, 4).position(), position(100));
    // A tall line that does not fit is not shown, so not counted.
    assert_eq!(View::new(vec![1, 1, 3], 4).position(), position(40));
}

#[test]
fn revealing_a_line_moves_only_when_it_is_not_shown() {
    let mut view = plain(10, 4);
    view.reveal(3);
    assert_eq!(view.top(), 0);
    view.reveal(5);
    assert_eq!(view.top(), 5);
    view.reveal(9);
    assert_eq!(view.top(), 8);
    view.reveal(1);
    assert_eq!(view.top(), 1);
    // A heading of two rows is shown whole.
    let mut view = View::new(vec![1, 1, 1, 2, 1], 3);
    view.reveal(3);
    assert_eq!(view.visible(), 3..5);
}

#[test]
fn going_to_a_line_puts_it_at_the_top_even_when_it_is_shown() {
    let mut view = plain(10, 4);
    view.go_to(2);
    assert_eq!(view.top(), 2);
    view.go_to(0);
    assert_eq!(view.top(), 0);
    // No further than the last page.
    view.go_to(9);
    assert_eq!(view.top(), 8);
}

#[test]
fn a_change_shown_whole_does_not_move_the_view() {
    let mut view = scrolled(plain(40, 10), &[Motion::Down(5)]);
    view.show_change(5..15);
    assert_eq!(view.top(), 5);
    view.show_change(9..9);
    assert_eq!(view.top(), 5);
    // At the bottom of the view, after its last line.
    view.show_change(15..15);
    assert_eq!(view.top(), 5);
}

#[test]
fn a_change_whose_start_is_shown_does_not_move_the_view() {
    let mut view = plain(60, 10);
    // Its first line on the last row, the rest below.
    view.show_change(9..20);
    assert_eq!(view.top(), 0);
}

#[test]
fn a_view_inside_a_change_taller_than_the_window_stays() {
    let mut view = scrolled(plain(60, 10), &[Motion::Down(25)]);
    view.show_change(20..45);
    assert_eq!(view.top(), 25);
    // Below its end, the view goes to its start.
    let mut view = scrolled(plain(60, 10), &[Motion::Down(45)]);
    view.show_change(20..45);
    // Two rows above it: a quarter of the window's ten.
    assert_eq!(view.top(), 18);
}

#[test]
fn a_change_away_from_the_view_goes_near_the_top() {
    let mut view = plain(60, 20);
    view.show_change(30..32);
    assert_eq!(view.top(), 27);
    // Above the view too, and cut off at the bottom.
    view.show_change(5..6);
    assert_eq!(view.top(), 2);
    view.show_change(5..30);
    assert_eq!(view.top(), 2);
    // A change without lines, as a removal is.
    view.show_change(25..25);
    assert_eq!(view.top(), 22);
    // Near the start, as many rows as there are.
    view.show_change(1..2);
    assert_eq!(view.top(), 0);
}

#[test]
fn a_short_window_shows_a_quarter_of_its_rows_above_a_change() {
    let mut view = plain(40, 8);
    view.show_change(20..21);
    assert_eq!(view.top(), 18);
    let mut view = plain(40, 3);
    view.show_change(20..21);
    assert_eq!(view.top(), 20);
}

#[test]
fn rows_above_a_change_count_by_height() {
    // A line of one row and a line of two above it.
    let mut heights = vec![1; 40];
    heights[12] = 2;
    let mut view = View::new(heights, 12);
    view.show_change(14..15);
    assert_eq!(view.top(), 12);
    // A tall line that does not fit in the rows above stays out.
    let mut heights = vec![1; 60];
    heights[30] = 4;
    let mut view = View::new(heights, 12);
    view.show_change(31..32);
    assert_eq!(view.top(), 31);
}

#[test]
fn a_change_at_the_end_shows_the_last_page() {
    let mut view = plain(40, 10);
    view.show_change(38..40);
    assert_eq!(view.visible(), 32..40);
    view = plain(40, 10);
    view.show_change(40..40);
    assert_eq!(view.visible(), 32..40);
}

/// A view of 40 lines in a window of 10 rows, with its top on `top`.
fn at(top: usize) -> View {
    let mut view = plain(40, 10);
    view.go_to(top);
    view
}

#[test]
fn a_picture_partly_below_scrolls_down_until_its_bottom_edge_is_at_the_bottom() {
    // Lines 14 to 19 start at row 6 of the window and end below it.
    let mut view = at(8);
    view.show_whole(14..20);
    assert_eq!(view.visible(), 10..20);
    view.show_whole(14..20);
    assert_eq!(view.top(), 10, "shown whole: no move");
    let mut view = at(8);
    view.show_whole(10..14);
    assert_eq!(view.top(), 8, "shown whole: no move");
    // The same with rows of other heights: a heading of two rows.
    let mut view = View::new(vec![1, 1, 1, 2, 1, 1, 1, 1], 5);
    view.go_to(2);
    view.show_whole(5..8);
    assert_eq!(view.visible(), 4..8);
}

#[test]
fn a_picture_partly_above_scrolls_up_until_its_top_edge_is_at_the_top() {
    let mut view = at(14);
    view.show_whole(12..16);
    assert_eq!(view.top(), 12);
}

#[test]
fn a_picture_wholly_outside_the_view_is_put_in_the_middle() {
    // Four lines in ten rows: three rows above, three below.
    let mut view = at(0);
    view.show_whole(20..24);
    assert_eq!(view.top(), 17);
    let mut view = at(30);
    view.show_whole(10..14);
    assert_eq!(view.top(), 7);
    // Six lines: two rows above, two below.
    let mut view = at(0);
    view.show_whole(20..26);
    assert_eq!(view.top(), 18);
}

#[test]
fn a_picture_near_an_end_of_the_document_is_centred_as_far_as_the_end_lets() {
    let mut view = at(20);
    view.show_whole(1..3);
    assert_eq!(view.top(), 0);
    let mut view = at(0);
    view.show_whole(37..40);
    assert_eq!(view.top(), view.last_top());
    assert!(view.visible().contains(&39), "{:?}", view.visible());
}

#[test]
fn a_picture_taller_than_the_view_has_its_top_at_the_top() {
    let mut view = at(0);
    view.show_whole(5..25);
    assert_eq!(view.top(), 5);
    // Partly shown below: its top goes to the top of the view.
    let mut view = at(0);
    view.show_whole(8..28);
    assert_eq!(view.top(), 8);
    // The reader is inside it: it does not scroll up so that the top
    // goes out.
    let mut view = at(12);
    view.show_whole(5..25);
    assert_eq!(view.top(), 12);
}

/// The lines of `source` laid out as the engine and window say, and the
/// map of them.
fn laid(source: &str, reading: bool, width: u16) -> (crate::typeset::driver::Stream, Map) {
    laid_at(source, reading, (width, 100))
}

/// [`laid`] with reading mode at `zoom` percent.
fn laid_at(
    source: &str,
    reading: bool,
    (width, zoom): (u16, u16),
) -> (crate::typeset::driver::Stream, Map) {
    use crate::render::Inputs;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::driver::{Content, content};
    let inputs = match reading {
        false => Inputs::new(Capabilities::kitty(width)),
        true => Inputs {
            reading: Some(crate::typeset::reading::engine::Reading {
                pictures: Default::default(),
                glyphs: Default::default(),
                fonts: std::rc::Rc::new(crate::typeset::reading::Fonts::fixture()),
                wide: crate::typeset::reading::page::Wide::Wrap,
                snap: false,
                zoom,
            }),
            ..Inputs::new(Capabilities {
                cell_pixels: Some((10, 20)),
                ..Capabilities::kitty(width)
            })
        },
    };
    let document = crate::typeset::parse::parse(source);
    let stream = content(Content::Document(&document), &inputs);
    let map = Map::new(&stream.lines, &stream.blocks);
    (stream, map)
}

const SOURCE: &str = "# Title\n\nFirst paragraph.\n\nSecond one, with more words.\n\n- a\n- b\n\n```\ncode\n```\n\nLast.\n";

#[test]
fn a_view_is_put_back_where_its_anchor_was_taken_in_the_same_lines() {
    for (reading, width) in [(false, 40), (false, 10), (true, 40)] {
        let (stream, map) = laid(SOURCE, reading, width);
        let heights = vec![1; stream.lines.len()];
        for top in 0..heights.len() {
            let mut view = View::new(heights.clone(), 5);
            view.go_to(top);
            let anchor = view.anchor(&map, false);
            let mut again = View::new(heights.clone(), 5);
            again.restore(&anchor, &map, true);
            assert_eq!(
                again.top(),
                view.top(),
                "{reading} {width} top {top}: {anchor:?}"
            );
        }
    }
}

#[test]
fn the_start_and_the_end_stay_where_they_are_and_a_blank_top_line_may_move_to_the_next_block() {
    let (stream, map) = laid(SOURCE, false, 40);
    let heights = vec![1; stream.lines.len()];
    let mut view = View::new(heights.clone(), 5);
    assert_eq!(view.anchor(&map, false), Anchor::Start);
    view.scroll(Motion::End);
    assert_eq!(view.anchor(&map, false), Anchor::End);
    // A blank line above a block has a place below it: a negative offset.
    let blank = (1..heights.len())
        .find(|&line| map.first_on(line).is_none())
        .unwrap();
    view.go_to(blank);
    let Anchor::At { lines, .. } = view.anchor(&map, false) else {
        panic!("an anchor at a place")
    };
    assert!(lines < 0, "{lines}");
}

#[test]
fn the_same_place_is_the_top_in_the_other_engine() {
    let (text, text_map) = laid(SOURCE, false, 40);
    let (page, page_map) = laid(SOURCE, true, 40);
    let heights = |count: usize| vec![1; count];
    for top in [3, 5, 7] {
        let mut view = View::new(heights(text.lines.len()), 5);
        view.go_to(top);
        let Anchor::At { pos, .. } = view.anchor(&text_map, false) else {
            panic!("an anchor at a place")
        };
        let mut other = View::new(heights(page.lines.len()), 5);
        other.restore(&view.anchor(&text_map, false), &page_map, false);
        // What the page shows at the top is of the same block as what
        // the text had there, or the next one.
        let first = (other.top()..page.lines.len()).find_map(|line| page_map.first_on(line));
        assert!(
            first.is_some_and(|first| first.path >= pos.path),
            "{top}: {first:?} {pos:?}"
        );
    }
}

/// The first line of `stream` whose text has `needle`.
fn line_with(stream: &crate::typeset::driver::Stream, needle: &str) -> usize {
    let at = (stream.lines.iter())
        .position(|line| crate::typeset::style::text(&line.spans).contains(needle));
    at.unwrap_or_else(|| panic!("no line with {needle:?}"))
}

/// A document like [`SOURCE`] with a longer first paragraph.
const LONGER: &str = "# Title\n\nFirst paragraph that is a good deal longer than it was before.\n\nSecond one, with more words.\n\n- a\n- b\n\n```\ncode\n```\n\nLast.\n";

#[test]
fn a_view_at_the_end_stays_on_the_last_page_of_other_lines() {
    let (stream, map) = laid(SOURCE, false, 40);
    let mut view = View::new(vec![1; stream.lines.len()], 5);
    view.scroll(Motion::End);
    let anchor = view.anchor(&map, false);
    let (other, other_map) = laid(SOURCE, false, 10);
    let mut again = View::new(vec![1; other.lines.len()], 5);
    again.restore(&anchor, &other_map, false);
    assert_eq!(again.visible().end, other.lines.len());
}

#[test]
fn a_blank_top_line_goes_to_the_start_of_the_next_block_in_lines_made_anew() {
    let (stream, map) = laid(SOURCE, false, 40);
    let blank = line_with(&stream, "First paragraph.") + 1;
    let mut view = View::new(vec![1; stream.lines.len()], 5);
    view.go_to(blank);
    let anchor = view.anchor(&map, false);
    let (other, other_map) = laid(LONGER, false, 20);
    let mut again = View::new(vec![1; other.lines.len()], 5);
    again.restore(&anchor, &other_map, false);
    assert_eq!(again.top(), line_with(&other, "Second one"));
}

#[test]
fn lines_made_again_for_the_same_content_keep_the_blank_top_line() {
    let (stream, map) = laid(SOURCE, false, 40);
    let blank = line_with(&stream, "First paragraph.") + 1;
    let mut view = View::new(vec![1; stream.lines.len()], 5);
    view.go_to(blank);
    let anchor = view.anchor(&map, false);
    // The block above the blank line grew by lines: the blank line, as
    // far above the same block, moves down with it.
    let (other, other_map) = laid(LONGER, false, 20);
    let mut again = View::new(vec![1; other.lines.len()], 5);
    again.restore(&anchor, &other_map, true);
    assert_eq!(again.top() + 1, line_with(&other, "Second one"));
}

#[test]
fn a_place_that_is_gone_puts_the_view_on_the_last_page() {
    let (stream, map) = laid(SOURCE, false, 40);
    let anchor = Anchor::At {
        pos: Pos::end(0, &[99]),
        lines: 0,
    };
    let mut view = View::new(vec![1; stream.lines.len()], 5);
    view.restore(&anchor, &map, false);
    assert_eq!(view.visible().end, stream.lines.len());
}

/// The line of the first match of `needle` that is the top of a view of
/// `old` laid out in one engine, and the line of the same place in
/// `new`, where the view is put by the anchor of the first aligned to
/// the second, as a reload does it, the document being `old` to `new`.
/// The needle is given with the one that the line is of in `new`.
fn reloaded(
    old: &str,
    new: &str,
    (needle, lands_on): (&str, &str),
    reading: bool,
) -> (usize, usize) {
    use crate::typeset::align::Align;
    use crate::typeset::compare::compare;
    use crate::typeset::parse::parse;
    use crate::typeset::text::DocumentText;
    let (old_document, new_document) = (parse(old), parse(new));
    let ((old_stream, old_map), (new_stream, new_map)) =
        (laid(old, reading, 40), laid(new, reading, 40));
    let old_text = DocumentText::new([&old_document]);
    let new_text = DocumentText::new([&new_document]);
    let top = old_map
        .line_of(&old_text.find(needle, false)[0].start)
        .unwrap();
    let mut view = View::new(vec![1; old_stream.lines.len()], 5);
    view.go_to(top);
    let Anchor::At { pos, lines } = view.anchor(&old_map, false) else {
        panic!("an anchor at a place")
    };
    let comparison = compare(&old_document, &new_document);
    let align = Align::new(0, &comparison, &new_text);
    let aligned = Anchor::At {
        pos: align.pos(&pos),
        lines,
    };
    let mut again = View::new(vec![1; new_stream.lines.len()], 5);
    again.restore(&aligned, &new_map, false);
    let wanted = new_map
        .line_of(&new_text.find(lands_on, false)[0].start)
        .unwrap();
    (again.top(), wanted)
}

const OLD: &str = "# Title\n\nFirst paragraph.\n\nSecond one, with more words.\n\nThird one.\n\nLast one.\n\nTail 1\n\nTail 2\n\nTail 3\n\nTail 4\n\nTail 5\n\nTail 6\n\nTail 7\n\nTail 8\n";

#[test]
fn a_reload_keeps_the_place_in_both_engines_when_a_block_is_added_above_it() {
    let new = OLD.replace("First paragraph.", "Added.\n\nFirst paragraph.");
    for reading in [false, true] {
        let (top, wanted) = reloaded(OLD, &new, ("Third one", "Third one"), reading);
        assert_eq!(top, wanted, "{reading}");
    }
}

#[test]
fn a_reload_keeps_the_place_in_both_engines_when_a_block_above_it_changed() {
    let new = OLD.replace("First paragraph.", "First paragraph, written anew.");
    for reading in [false, true] {
        let (top, wanted) = reloaded(OLD, &new, ("Third one", "Third one"), reading);
        assert_eq!(top, wanted, "{reading}");
    }
}

#[test]
fn a_reload_keeps_the_place_in_both_engines_when_a_block_above_it_is_removed() {
    let new = OLD.replace("First paragraph.\n\n", "");
    for reading in [false, true] {
        let (top, wanted) = reloaded(OLD, &new, ("Third one", "Third one"), reading);
        assert_eq!(top, wanted, "{reading}");
    }
}

#[test]
fn a_reload_in_both_engines_goes_to_the_next_block_when_the_top_block_is_removed() {
    let new = OLD.replace("Third one.\n\n", "");
    for reading in [false, true] {
        // The view was at the removed block, and is at "Last one" now.
        let (top, wanted) = reloaded(OLD, &new, ("Third one", "Last one"), reading);
        assert_eq!(top, wanted, "{reading}");
    }
}

/// A long paragraph of numbered words and then a code block, to be laid
/// out at different widths.
fn words_then_code() -> String {
    let words: Vec<String> = (0..400).map(|n| format!("w{n}")).collect();
    let code: Vec<String> = (0..40).map(|n| format!("line{n}")).collect();
    format!("{}\n\n```\n{}\n```\n", words.join(" "), code.join("\n"))
}

/// The byte of the text that the first place on `line` of `map` is at.
fn byte_on(map: &Map, line: usize) -> usize {
    match map.first_on(line).map(|pos| &pos.within) {
        Some(crate::typeset::position::Within::Text { offset, .. }) => *offset,
        other => panic!("no text on line {line}: {other:?}"),
    }
}

/// The row of the lines that the word `word` of `source` is shown on,
/// as the search finds it.
fn row_of_word(
    source: &str,
    stream: &crate::typeset::driver::Stream,
    map: &Map,
    word: &str,
) -> usize {
    use crate::typeset::text::DocumentText;
    let document = crate::typeset::parse::parse(source);
    let found = DocumentText::new([&document]).find(word, false)[0].clone();
    let parts = map.locate(&stream.lines, &found).expect("shown");
    map.start(&parts[0].piece).expect("a start") + parts[0].rows.start
}

#[test]
fn a_page_laid_out_again_keeps_the_text_at_the_top_inside_a_paragraph() {
    let source = words_then_code();
    let (wide, wide_map) = laid(&source, true, 100);
    let (narrow, narrow_map) = laid(&source, true, 50);
    for word in ["w60", "w130", "w200", "w300"] {
        // The top is the row that has the word, and the word is on the
        // same row or a few below the top after the page is laid out
        // again: not a paragraph's worth of rows away.
        let top = row_of_word(&source, &wide, &wide_map, word);
        let mut view = View::new(vec![1; wide.lines.len()], 5);
        view.go_to(top);
        let mut again = View::new(vec![1; narrow.lines.len()], 5);
        again.restore(&view.anchor(&wide_map, false), &narrow_map, false);
        let at = row_of_word(&source, &narrow, &narrow_map, word);
        assert!(
            (again.top()..again.top() + 5).contains(&at),
            "{word}: row {top} became row {}, the word is on row {at}",
            again.top()
        );
    }
}

#[test]
fn a_page_laid_out_at_another_zoom_keeps_the_text_at_the_top_inside_a_paragraph() {
    let source = words_then_code();
    for (from, to) in [(100, 150), (150, 100), (100, 50)] {
        let (before, before_map) = laid_at(&source, true, (100, from));
        let (after, after_map) = laid_at(&source, true, (100, to));
        assert_ne!(before.lines.len(), after.lines.len(), "{from} % to {to} %");
        for word in ["w60", "w130", "w200", "w300"] {
            let top = row_of_word(&source, &before, &before_map, word);
            let mut view = View::new(vec![1; before.lines.len()], 5);
            view.go_to(top);
            let mut again = View::new(vec![1; after.lines.len()], 5);
            again.restore(&view.anchor(&before_map, false), &after_map, false);
            let at = row_of_word(&source, &after, &after_map, word);
            // A row above the top is right too: a line of text is not a
            // row high, and the top is the first line that starts on a
            // row, not the line the word was in.
            assert!(
                (again.top().saturating_sub(1)..again.top() + 5).contains(&at),
                "{word}, {from} % to {to} %: row {top} became row {}, the word is on row {at}",
                again.top()
            );
        }
    }
}

#[test]
fn a_top_inside_a_paragraph_of_text_mode_is_the_row_of_that_text_in_reading_mode() {
    let source = words_then_code();
    let (text, text_map) = laid(&source, false, 40);
    let (page, page_map) = laid(&source, true, 100);
    for top in [4, 9] {
        let mut view = View::new(vec![1; text.lines.len()], 5);
        view.go_to(top);
        let mut other = View::new(vec![1; page.lines.len()], 5);
        other.restore(&view.anchor(&text_map, false), &page_map, false);
        let (old, new) = (byte_on(&text_map, top), byte_on(&page_map, other.top()));
        assert!(
            old.abs_diff(new) <= 150,
            "text row {top} at byte {old} came to row {} at byte {new}",
            other.top()
        );
    }
}

#[test]
fn a_top_in_a_code_block_stays_on_its_line_when_the_page_is_laid_out_again() {
    let source = words_then_code();
    let (wide, wide_map) = laid(&source, true, 100);
    let (narrow, narrow_map) = laid(&source, true, 50);
    let top = wide.lines.len() - 15;
    let mut view = View::new(vec![1; wide.lines.len()], 5);
    view.go_to(top);
    let mut again = View::new(vec![1; narrow.lines.len()], 5);
    again.restore(&view.anchor(&wide_map, false), &narrow_map, false);
    assert_eq!(narrow_map.first_on(again.top()), wide_map.first_on(top));
}
