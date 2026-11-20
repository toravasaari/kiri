//! The bar's rows in the colours of a theme: what each kind of row, the
//! edge of the current one and the border look like, with and without the
//! focus.

use super::*;
use crate::typeset::color::Color;

const fn rgb(r: u8, g: u8, b: u8) -> Option<Color> {
    Some(Color::Rgb(r, g, b))
}

const BAR: BarColors = BarColors {
    background: rgb(10, 10, 10),
    title: rgb(1, 1, 200),
    text: rgb(2, 2, 200),
    glyph: rgb(3, 3, 200),
    current: rgb(4, 4, 200),
    current_background: rgb(40, 40, 40),
    selected: rgb(5, 5, 200),
    border: rgb(6, 6, 200),
    track: rgb(20, 20, 20),
    thumb: rgb(90, 90, 90),
};

fn contents() -> Contents {
    // One, Two and Four are of levels 1, 2 and 2; Three is of level 3.
    super::tests::contents_of("# One\n\n## Two\n\n### Three\n\n## Four\n")
}

fn looks() -> Looks {
    Looks::coloured(&BAR, Style::default())
}

/// The rows of the bar of `contents`, 16 columns and 6 rows.
fn drawn(current: Option<usize>, selected: Option<usize>) -> Vec<Line> {
    let names = vec!["a.md".to_owned()];
    rows(
        &contents(),
        (&Folds::default(), &names),
        (current, selected),
        (16, 6),
        (&looks(), 0),
    )
}

fn text(line: &Line) -> String {
    line.spans.iter().map(|span| span.text.as_str()).collect()
}

/// The span of `line` that holds `needle`.
fn span<'a>(line: &'a Line, needle: &str) -> &'a Span {
    (line.spans.iter())
        .find(|span| span.text.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} in {:?}", text(line)))
}

fn on(style: &Style, fg: Option<Color>, bg: Option<Color>) -> bool {
    style.fg == fg && style.bg == bg
}

#[test]
fn every_row_ends_in_the_border_also_the_blank_ones() {
    for line in drawn(Some(0), None) {
        let text = text(&line);
        assert!(text.ends_with('▕'), "{text:?}");
        assert_eq!(text.chars().count(), 16, "{text:?}");
        assert!(on(&span(&line, "▕").style, BAR.border, BAR.background));
    }
}

#[test]
fn the_border_is_in_the_selection_s_colour_while_the_bar_has_the_focus() {
    for line in drawn(Some(0), Some(1)) {
        assert!(on(&span(&line, "▕").style, BAR.selected, BAR.background));
    }
}

#[test]
fn the_row_of_the_current_heading_has_an_edge_and_a_background_of_its_own() {
    let lines = drawn(Some(2), None);
    assert_eq!(text(&lines[2]), "▌      Three   ▕");
    let edge = span(&lines[2], "▌");
    assert!(on(&edge.style, BAR.current, BAR.current_background));
    // The whole row, its trailing cells too.
    let last = &lines[2].spans[lines[2].spans.len() - 2];
    assert_eq!(last.style.bg, BAR.current_background, "{last:?}");
    assert!(span(&lines[2], "Three").style.bold);
    // The others have none.
    for other in [0, 1, 3] {
        assert!(
            text(&lines[other]).starts_with(' '),
            "{}",
            text(&lines[other])
        );
        assert_eq!(span(&lines[other], " ").style.bg, BAR.background);
    }
}

#[test]
fn the_top_levels_are_titles_in_bold_and_the_deeper_ones_text() {
    let lines = drawn(None, None);
    for (row, name) in [(0, "One"), (1, "Two"), (3, "Four")] {
        let style = &span(&lines[row], name).style;
        assert!(style.bold, "{name}");
        assert!(on(style, BAR.title, BAR.background), "{name}");
    }
    let style = &span(&lines[2], "Three").style;
    assert!(!style.bold);
    assert!(on(style, BAR.text, BAR.background));
}

#[test]
fn the_fold_glyph_has_the_glyph_colour() {
    let lines = drawn(None, None);
    assert!(on(&span(&lines[0], "▾").style, BAR.glyph, BAR.background));
}

#[test]
fn the_empty_rows_below_the_entries_are_in_the_bar_s_background() {
    let lines = drawn(Some(0), None);
    for line in &lines[4..] {
        assert_eq!(text(line), format!("{}▕", " ".repeat(15)));
        assert_eq!(line.spans[0].style.bg, BAR.background);
    }
}

#[test]
fn the_selected_row_is_in_reverse_in_the_selected_colour_and_not_in_the_edge_cell() {
    let lines = drawn(Some(0), Some(1));
    let style = &span(&lines[1], "Two").style;
    assert!(style.reverse && style.bold);
    assert!(on(style, BAR.selected, BAR.background));
    assert!(!text(&lines[1]).starts_with('▌'));
    assert!(
        !span(&lines[1], " ").style.reverse,
        "the edge cell is the bar's"
    );
    // The row of the heading the reader is in keeps its look.
    assert!(text(&lines[0]).starts_with('▌'));
    assert!(!span(&lines[0], "One").style.reverse);
    assert_eq!(span(&lines[0], "One").style.bg, BAR.current_background);
}

#[test]
fn the_selection_on_the_current_heading_is_the_selected_look_with_the_edge() {
    let lines = drawn(Some(2), Some(2));
    assert!(text(&lines[2]).starts_with('▌'));
    let style = &span(&lines[2], "Three").style;
    assert!(style.reverse);
    assert_eq!(style.fg, BAR.selected);
}

#[test]
fn a_bar_without_a_background_leaves_the_terminal_s() {
    let colors = BarColors {
        background: None,
        current_background: None,
        ..BAR
    };
    let names = vec!["a.md".to_owned()];
    let lines = rows(
        &contents(),
        (&Folds::default(), &names),
        (Some(2), None),
        (16, 6),
        (&Looks::coloured(&colors, Style::default()), 0),
    );
    assert!(text(&lines[2]).starts_with('▌'));
    assert_eq!(span(&lines[2], "Three").style.bg, None);
    assert_eq!(span(&lines[5], "▕").style.fg, BAR.border);
}

#[test]
fn a_row_cut_to_a_narrow_bar_still_ends_in_the_border() {
    let names = vec!["a.md".to_owned()];
    for columns in 1..6 {
        let lines = rows(
            &contents(),
            (&Folds::default(), &names),
            (Some(2), Some(1)),
            (columns, 4),
            (&looks(), 0),
        );
        for line in lines {
            let text = text(&line);
            assert!(text.ends_with('▕'), "{columns}: {text:?}");
            assert_eq!(text.chars().count(), columns, "{columns}: {text:?}");
        }
    }
}

#[test]
fn a_plain_bar_has_no_edge_and_no_border() {
    let mark = Style {
        bold: true,
        ..Style::default()
    };
    let looks = Looks::plain(Style::default(), Style::default(), mark);
    let names = vec!["a.md".to_owned()];
    let lines = rows(
        &contents(),
        (&Folds::default(), &names),
        (Some(2), None),
        (16, 4),
        (&looks, 0),
    );
    for line in &lines {
        let text = text(line);
        assert!(!text.contains('▌') && !text.contains('▕'), "{text:?}");
        assert_eq!(text.chars().count(), 16);
    }
    assert!(span(&lines[2], "Three").style.bold);
    assert!(!span(&lines[2], "Three").style.reverse);
}

#[test]
fn a_plain_bar_shows_the_selection_in_reverse_video() {
    let mark = Style {
        bold: true,
        ..Style::default()
    };
    let looks = Looks::plain(Style::default(), Style::default(), mark);
    let names = vec!["a.md".to_owned()];
    let lines = rows(
        &contents(),
        (&Folds::default(), &names),
        (Some(2), Some(1)),
        (16, 4),
        (&looks, 0),
    );
    assert!(span(&lines[1], "Two").style.reverse);
    // The entry the reader is in is quiet, not reversed, and no edge.
    assert!(!span(&lines[2], "Three").style.reverse);
    assert!(span(&lines[2], "Three").style.italic);
}

/// The cell of the scroll bar's column, the one before the border's.
fn bar_cell(line: &Line) -> &Span {
    &line.spans[line.spans.len() - 2]
}

#[test]
fn a_list_longer_than_the_rows_has_a_scroll_bar_cell_before_the_border_in_the_bar_s_colours() {
    let names = vec!["a.md".to_owned()];
    // Four entries in two rows.
    let lines = rows(
        &contents(),
        (&Folds::default(), &names),
        (Some(0), None),
        (16, 2),
        (&looks(), 0),
    );
    let (first, second) = (bar_cell(&lines[0]), bar_cell(&lines[1]));
    // Two rows are too few to leave half a row free. The thumb is the upper
    // half rows: a full block in the thumb's colour, then one in the track's.
    assert_eq!((first.text.as_str(), second.text.as_str()), ("█", "█"));
    assert!(on(&first.style, rgb(90, 90, 90), rgb(90, 90, 90)));
    assert!(on(&second.style, rgb(20, 20, 20), rgb(20, 20, 20)));
    assert_eq!(text(&lines[0]).chars().count(), 16);
    assert!(text(&lines[0]).ends_with("█▕"), "{}", text(&lines[0]));
    // A plain bar has no border: the cell is next to an empty column.
    let plain = Looks::plain(Style::default(), Style::default(), Style::default());
    let lines = rows(
        &contents(),
        (&Folds::default(), &names),
        (Some(0), None),
        (16, 2),
        (&plain, 0),
    );
    assert!(text(&lines[0]).ends_with("█ "), "{}", text(&lines[0]));
    assert_eq!(text(&lines[0]).chars().count(), 16);
}

#[test]
fn a_list_that_fits_the_rows_has_none_and_the_text_keeps_its_width() {
    let names = vec!["a.md".to_owned()];
    let lines = rows(
        &contents(),
        (&Folds::default(), &names),
        (Some(0), None),
        (16, 6),
        (&looks(), 0),
    );
    assert!(text(&lines[0]).ends_with(" ▕"), "{}", text(&lines[0]));
    // The edge, the glyph, the text, the fill and the border; the empty
    // indent has no span of its own.
    let spans: Vec<&str> = lines[0].spans.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(spans.len(), 5, "{spans:?}");
}

#[test]
fn the_scroll_bar_colours_come_from_the_derived_pair_or_the_helps_greys() {
    let looks = looks();
    assert_eq!(looks.scrollbar.fg, rgb(90, 90, 90));
    assert_eq!(looks.scrollbar.bg, rgb(20, 20, 20));
    let bare = BarColors {
        track: None,
        thumb: None,
        ..BAR
    };
    let looks = Looks::coloured(&bare, Style::default());
    assert_eq!(looks.scrollbar.fg, Some(Color::Palette(250)));
    assert_eq!(looks.scrollbar.bg, Some(Color::Palette(237)));
}

const PAGE: Option<Color> = rgb(10, 10, 10);
const TINT: Option<Color> = rgb(40, 40, 40);
const THUMB: Option<Color> = rgb(90, 90, 90);
const TRACK: Option<Color> = rgb(20, 20, 20);

/// The bar's cell for row `row` of `rows` and a thumb at `(start, size)`.
fn cell(row: (usize, usize), thumb: (usize, usize)) -> Span {
    let looks = looks();
    super::bar_cell(row, thumb, (&looks.page, &looks.scrollbar, looks.plain))
}

#[test]
fn a_cell_of_the_scroll_bar_is_drawn_from_its_two_half_rows() {
    // 11 rows, a thumb in the half rows 2 to 4: the upper half of the first
    // row is free, the lower one the track.
    let at = |row| cell((row, 11), (2, 3));
    let (first, thumb, mixed, track, last) = (at(0), at(1), at(2), at(3), at(10));
    assert_eq!(first.text, "▄");
    assert!(on(&first.style, TRACK, PAGE), "{:?}", first.style);
    // Both halves the thumb's: a full block in its colour, on that colour
    // too, as xterm draws the glyph short of the cell.
    assert_eq!(thumb.text, "█");
    assert!(on(&thumb.style, THUMB, THUMB), "{:?}", thumb.style);
    // The thumb above, the track below: the upper half block, the thumb as
    // its foreground and the track as its background.
    assert_eq!(mixed.text, "▀");
    assert!(on(&mixed.style, THUMB, TRACK), "{:?}", mixed.style);
    assert_eq!(track.text, "█");
    assert!(on(&track.style, TRACK, TRACK), "{:?}", track.style);
    // The lower half of the last row is free.
    assert_eq!(last.text, "▀");
    assert!(on(&last.style, TRACK, PAGE), "{:?}", last.style);
    // The thumb below, the track above, as the lower half is the thumb's.
    let lower = cell((3, 11), (7, 3));
    assert_eq!(lower.text, "▀");
    assert!(on(&lower.style, TRACK, THUMB), "{:?}", lower.style);
}

#[test]
fn the_thumb_at_either_end_meets_the_free_half_row_with_the_bar_s_background() {
    let top = cell((0, 11), (1, 7));
    assert_eq!(top.text, "▄");
    assert!(on(&top.style, THUMB, PAGE), "{:?}", top.style);
    let bottom = cell((10, 11), (14, 7));
    assert_eq!(bottom.text, "▀");
    assert!(on(&bottom.style, THUMB, PAGE), "{:?}", bottom.style);
}

#[test]
fn a_bar_of_two_rows_has_no_free_half_rows() {
    let (first, second) = (cell((0, 2), (1, 2)), cell((1, 2), (1, 2)));
    assert_eq!(first.text, "▀");
    assert!(on(&first.style, TRACK, THUMB), "{:?}", first.style);
    assert_eq!(second.text, "▀");
    assert!(on(&second.style, THUMB, TRACK), "{:?}", second.style);
}

/// A document of six headings of one level, and the rows of a bar of three
/// rows that shows them from `first` with the entry `current` the
/// reader's: the rows' tint is the current one's.
fn tinted(current: usize, first: usize) -> Vec<Line> {
    let source: String = (0..6).map(|n| format!("# H{n}\n\n")).collect();
    let names = vec!["a.md".to_owned()];
    rows(
        &super::tests::contents_of(&source),
        (&Folds::default(), &names),
        (Some(current), None),
        (16, 3),
        (&looks(), first),
    )
}

#[test]
fn the_free_half_row_of_a_tinted_row_at_the_top_has_the_bar_s_background_not_the_tint() {
    let lines = tinted(0, 0);
    // The current row has the tint, up to the cell ahead of the bar.
    let spans = &lines[0].spans;
    assert_eq!(spans[spans.len() - 4].style.bg, TINT);
    let cell = bar_cell(&lines[0]);
    assert_eq!(cell.text, "▄");
    assert!(on(&cell.style, THUMB, PAGE), "{:?}", cell.style);
    // The other rows are not tinted: the thumb above the track.
    assert!(on(&bar_cell(&lines[1]).style, THUMB, TRACK));
}

#[test]
fn the_free_half_row_of_a_tinted_row_at_the_bottom_has_the_bar_s_background_not_the_tint() {
    let lines = tinted(5, 3);
    let cell = bar_cell(&lines[2]);
    assert_eq!(cell.text, "▀");
    assert!(on(&cell.style, THUMB, PAGE), "{:?}", cell.style);
    assert_ne!(cell.style.bg, TINT);
}

#[test]
fn the_cell_ahead_of_the_scroll_bar_is_the_bar_s_background_where_the_row_is_tinted() {
    for (current, first, at) in [(0, 0, 0), (5, 3, 2)] {
        let line = &tinted(current, first)[at];
        let spans = &line.spans;
        let gap = &spans[spans.len() - 3];
        assert_eq!(gap.text, " ");
        assert_eq!(gap.style, looks().page);
        assert!(on(&gap.style, None, PAGE), "{:?}", gap.style);
        // The fill before it has the row's tint.
        let fill = &spans[spans.len() - 4];
        assert_eq!(fill.style.bg, TINT, "{:?}", text(line));
        assert_eq!(text(line).chars().count(), 16);
    }
}

#[test]
fn a_row_with_a_scroll_bar_has_two_columns_less_and_the_ellipsis_ends_before_the_gap() {
    let looks = looks();
    let long = "x".repeat(40);
    let bar = Span::new("█", looks.scrollbar.clone());
    let gap = Span::new(" ", looks.page.clone());
    let border = looks.border.as_ref().map(|(plain, _)| plain);
    let with = |bar: Option<(Span, Span)>| {
        let edge = Span::new(" ", looks.page.clone());
        super::row(
            ("", ' ', &long),
            20,
            (&looks.normal, &looks.normal.title, border),
            (edge, bar),
        )
    };
    let (barless, barred) = (with(None), with(Some((gap.clone(), bar.clone()))));
    let at = |line: &Line| text(line).chars().position(|c| c == '…').unwrap();
    assert_eq!(at(&barless), 18);
    assert_eq!(at(&barred), 16);
    assert!(text(&barred).ends_with("… █▕"), "{}", text(&barred));
    assert_eq!(text(&barred).chars().count(), 20);
    assert_eq!(barred.spans[barred.spans.len() - 3], gap);
    assert_eq!(barred.spans[barred.spans.len() - 2], bar);
    // A short text is filled up to the gap.
    let short = super::row(
        ("", ' ', "a"),
        20,
        (&looks.normal, &looks.normal.title, border),
        (Span::new(" ", looks.page.clone()), Some((gap, bar))),
    );
    assert!(
        text(&short).ends_with("a              █▕"),
        "{}",
        text(&short)
    );
}

#[test]
fn a_plain_bar_keeps_its_drag_column_after_the_scroll_bar_cell() {
    let plain = Looks::plain(Style::default(), Style::default(), Style::default());
    let long = "x".repeat(40);
    let bar = Span::new("█", Style::default());
    let line = super::row(
        ("", ' ', &long),
        20,
        (&plain.normal, &plain.normal.title, None),
        (
            Span::new(" ", Style::default()),
            Some((Span::new(" ", Style::default()), bar)),
        ),
    );
    // The text, the gap, the scroll bar and the empty column to drag.
    assert!(text(&line).ends_with("… █ "), "{}", text(&line));
    assert_eq!(text(&line).chars().count(), 20);
    assert_eq!(text(&line).chars().position(|c| c == '…'), Some(16));
}

#[test]
fn a_bar_of_three_columns_or_fewer_has_no_scroll_bar_cell() {
    let looks = looks();
    let line = super::row(
        ("", ' ', "abc"),
        3,
        (&looks.normal, &looks.normal.title, None),
        (
            Span::new(" ", looks.page.clone()),
            Some((
                Span::new(" ", looks.page.clone()),
                Span::new("█", looks.scrollbar.clone()),
            )),
        ),
    );
    assert!(!text(&line).contains('█'), "{}", text(&line));
}

#[test]
fn a_plain_scroll_bar_draws_the_thumb_in_blocks_and_leaves_the_track_blank() {
    let plain = Looks::plain(Style::default(), Style::default(), Style::default());
    let at = |row| {
        super::bar_cell(
            (row, 11),
            (2, 3),
            (&plain.page, &plain.scrollbar, plain.plain),
        )
    };
    let texts: Vec<String> = (0..11).map(|row| at(row).text).collect();
    // Half rows 2 to 4: all of row 1 and the upper half of row 2; the rest
    // is blank, the free half rows too.
    assert_eq!(
        texts,
        [" ", "█", "▀", " ", " ", " ", " ", " ", " ", " ", " "]
    );
}

#[test]
fn a_long_heading_in_a_plain_bar_leaves_the_last_column_blank() {
    let plain = Looks::plain(Style::default(), Style::default(), Style::default());
    let long = "x".repeat(40);
    let line = super::row(
        ("", ' ', &long),
        20,
        (&plain.normal, &plain.normal.title, None),
        (Span::new(" ", Style::default()), None),
    );
    // The text is cut with an ellipsis before the column the mouse drags.
    assert!(text(&line).ends_with("… "), "{}", text(&line));
    assert_eq!(text(&line).chars().count(), 20);
    assert_eq!(text(&line).chars().position(|c| c == '…'), Some(18));
}
