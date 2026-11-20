//! The contents bar with several files: a blank row, a gap, before each
//! file entry but the first. It is a row of the list that is no entry: it is
//! never selected, the keys skip it, a click on it does nothing, and it
//! counts in the scroll bar and the scrolling.

use super::*;
use crate::typeset::parse::parse;
use crate::typeset::targets::targets;

/// The names of the three files of [`three`].
fn names() -> Vec<String> {
    ["one.md", "two.md", "three.md"].map(str::to_owned).to_vec()
}

/// The contents of three files: `one.md` with `A` and `B` under it, `two.md`
/// with `C`, `three.md` with `D`; the entries 0 to 7 are `one.md`, `A`, `B`,
/// `two.md`, `C`, `three.md`, `D`... see the list in each test.
fn three() -> Contents {
    let documents = [parse("# A\n\n## B\n"), parse("# C\n"), parse("# D\n")];
    let found = targets(documents.iter());
    let named = names();
    crate::typeset::contents::contents(
        named.iter().map(|name| Some(name.as_str())).zip(&documents),
        &found,
    )
}

/// The text of each row of the bar, without the blanks at the end.
fn texts(contents: &Contents, folds: &Folds, size: (usize, usize), first: usize) -> Vec<String> {
    let looks = Looks::plain(Style::default(), Style::default(), Style::default());
    let lines = rows(
        contents,
        (folds, &names()),
        (None, None),
        size,
        (&looks, first),
    );
    let text = |line: &Line| line.spans.iter().map(|span| span.text.as_str()).collect();
    let text: Vec<String> = lines.iter().map(text).collect();
    text.iter()
        .map(|row: &String| row.trim_end().to_owned())
        .collect()
}

#[test]
fn rows_with_several_files_have_a_blank_row_before_each_file_but_the_first() {
    let shown = texts(&three(), &Folds::default(), (20, 10), 0);
    assert_eq!(
        shown,
        [
            " ▾ one.md",
            "   ▾ A",
            "       B",
            "",
            " ▾ two.md",
            "     C",
            "",
            " ▾ three.md",
            "     D",
            "",
        ]
    );
}

#[test]
fn a_folded_file_keeps_its_gap_before_it() {
    let contents = three();
    let mut folds = Folds::default();
    folds.fold("two.md", &contents.entries()[3], true);
    let shown = texts(&contents, &folds, (20, 9), 0);
    assert_eq!(
        shown[2..8],
        ["       B", "", " ▸ two.md", "", " ▾ three.md", "     D"].map(str::to_owned)
    );
}

#[test]
fn a_bar_of_one_file_has_no_gap() {
    let document = parse("# A\n\n## B\n");
    let found = targets([&document]);
    let named = crate::typeset::contents::contents([(None, &document)], &found);
    let size = (20, 4);
    let one = vec!["a.md".to_owned()];
    assert_eq!(
        listed(&named, (&Folds::default(), &one)).len(),
        named.entries().len()
    );
    let shown = texts(&named, &Folds::default(), size, 0);
    assert_eq!(shown, [" ▾ A", "     B", "", ""]);
}

#[test]
fn the_list_counts_the_gaps() {
    let contents = three();
    // Seven entries and two gaps.
    assert_eq!(listed(&contents, (&Folds::default(), &names())).len(), 9);
}

#[test]
fn the_keys_skip_the_gap() {
    let contents = three();
    let go = |key, from, rows| {
        let mut folds = Folds::default();
        press(key, from, &contents, (&mut folds, &names()), rows)
    };
    // The list: 0 1 2 gap 3 4 gap 5 6.
    assert_eq!(go(BarKey::Down, 2, 4), Chosen::Selected(3));
    assert_eq!(go(BarKey::Up, 3, 4), Chosen::Selected(2));
    assert_eq!(go(BarKey::Up, 5, 4), Chosen::Selected(4));
    assert_eq!(go(BarKey::First, 4, 4), Chosen::Selected(0));
    assert_eq!(go(BarKey::Last, 0, 4), Chosen::Selected(6));
    // A page is as many list rows as the bar has, the gaps counted: four
    // rows down from `one.md` is the gap, so the file after it.
    assert_eq!(go(BarKey::PageDown, 0, 4), Chosen::Selected(3));
    assert_eq!(go(BarKey::PageDown, 1, 4), Chosen::Selected(4));
    assert_eq!(go(BarKey::PageDown, 4, 4), Chosen::Selected(6));
    // Up: from `three.md` (row 7) four rows up is row 3, the gap, and the
    // entry before it.
    assert_eq!(go(BarKey::PageUp, 5, 4), Chosen::Selected(2));
    assert_eq!(go(BarKey::PageUp, 6, 4), Chosen::Selected(3));
    assert_eq!(go(BarKey::PageUp, 1, 4), Chosen::Selected(0));
}

#[test]
fn a_click_on_a_gap_chooses_nothing_and_the_rows_around_it_choose_their_entry() {
    let contents = three();
    let folds = Folds::default();
    let at = |row| hit(&contents, (&folds, &names()), 0, (20, 5, row));
    // Rows from 1: 1 `one.md`, 2 `A`, 3 `B`, 4 the gap, 5 `two.md`.
    assert_eq!(at(3), Some(Clicked::Entry(2)));
    assert_eq!(at(4), None);
    assert_eq!(at(5), Some(Clicked::Entry(3)));
    // On the gap's fold-glyph column too.
    assert_eq!(hit(&contents, (&folds, &names()), 0, (20, 2, 4)), None);
}

#[test]
fn the_thumb_and_a_drag_of_it_work_on_the_rows_with_the_gaps() {
    let contents = three();
    let all = listed(&contents, (&Folds::default(), &names())).len();
    // Nine rows in a bar of five: from the second half row, a thumb of
    // `2 * 3 * 5 / 9` rounded, at the top, and the last place at the end.
    assert_eq!(thumb(all, 5, 0), thumb(9, 5, 0));
    assert_eq!(thumb(all, 5, 0), Some((1, 4)));
    assert_eq!(thumb(all, 5, 4), Some((5, 4)));
    // The last place of the list is the row 4, and a drag to the end of the
    // track gets there.
    assert_eq!(dragged_to((all, 5), 0, 5), 4);
}

#[test]
fn the_list_moves_to_bring_the_reader_s_entry_and_the_gap_above_it_into_view() {
    let contents = three();
    let list = listed(&contents, (&Folds::default(), &names()));
    // Rows 3 of nine: `two.md` is the fifth row, the gap the fourth.
    // Above the list's top (stored 5): the gap comes with the file.
    assert_eq!(settle(&list, (Some(3), None), 3, (None, 5)), 3);
    // A heading above its top has no gap above it.
    assert_eq!(settle(&list, (Some(4), None), 3, (None, 5)), 5);
    // Below: the file entry (row 7, list row 6) goes to the last row.
    assert_eq!(settle(&list, (Some(5), None), 3, (None, 0)), 5);
    // `D` at the end (list row 8, last row of the bar).
    assert_eq!(settle(&list, (Some(6), None), 3, (None, 0)), 6);
    // The reader's own scroll is kept up to the end of the list.
    assert_eq!(settle(&list, (Some(0), None), 3, (Some(99), 0)), 6);
}

// The file entry's name and the empty cells after it are underlined.

fn bar_colors() -> BarColors {
    let rgb = |r, g, b| Some(crate::typeset::color::Color::Rgb(r, g, b));
    BarColors {
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
    }
}

/// One character for each cell of `line`: `u` where it is underlined.
fn marks(line: &Line) -> String {
    let mut marks = String::new();
    for span in &line.spans {
        let mark = if span.style.underline == crate::typeset::color::Underline::Single {
            'u'
        } else {
            '.'
        };
        marks.extend(std::iter::repeat_n(mark, span.text.width()));
    }
    marks
}

/// The rows of the bar of `three()`, `columns` wide and `rows` tall.
fn drawn(
    looks: &Looks,
    (current, selected): (Option<usize>, Option<usize>),
    (columns, rows_shown): (usize, usize),
) -> Vec<Line> {
    rows(
        &three(),
        (&Folds::default(), &names()),
        (current, selected),
        (columns, rows_shown),
        (looks, 0),
    )
}

#[test]
fn a_file_s_name_and_the_empty_cells_after_it_are_underlined_up_to_the_border() {
    let looks = Looks::coloured(&bar_colors(), Style::default());
    let lines = drawn(&looks, (None, None), (20, 10));
    // The edge cell, the glyph and the space are not; the name and the
    // fill up to the border's column are.
    assert_eq!(marks(&lines[0]), format!("...{}.", "u".repeat(16)));
    assert_eq!(marks(&lines[4]), format!("...{}.", "u".repeat(16)));
    // A heading, a gap and the rows below the entries have none.
    for row in [1, 2, 3, 5, 6, 9] {
        assert_eq!(marks(&lines[row]), ".".repeat(20), "row {row}");
    }
}

#[test]
fn the_underline_stops_before_the_gap_column_and_the_scroll_bar() {
    let looks = Looks::coloured(&bar_colors(), Style::default());
    // Five rows of nine: a scroll bar; `one.md` is the first row.
    let lines = drawn(&looks, (None, None), (20, 5));
    assert_eq!(marks(&lines[0]), format!("...{}...", "u".repeat(14)));
}

#[test]
fn the_underline_is_in_the_colour_of_the_headings() {
    let colors = bar_colors();
    let looks = Looks::coloured(&colors, Style::default());
    let lines = drawn(&looks, (None, None), (20, 10));
    let name = lines[0].spans.iter().find(|s| s.text.contains("one.md"));
    assert_eq!(name.unwrap().style.fg, colors.title);
    // The fill after the name has the colour too, as the underline takes it
    // from the text, and keeps the row's background.
    let fill = lines[0]
        .spans
        .iter()
        .rev()
        .find(|s| s.style.underline != Default::default());
    let fill = fill.unwrap();
    assert_eq!(
        (fill.style.fg, fill.style.bg),
        (colors.title, colors.background)
    );
}

#[test]
fn the_current_and_the_selected_file_keep_their_tint_and_reverse_with_the_underline() {
    let colors = bar_colors();
    let looks = Looks::coloured(&colors, Style::default());
    // The reader is in `two.md`, which is row 4 of the list.
    let lines = drawn(&looks, (Some(3), None), (20, 10));
    assert_eq!(marks(&lines[4]), format!("...{}.", "u".repeat(16)));
    assert!(lines[4].spans.iter().any(
        |s| s.style.bg == colors.current_background && s.style.underline != Default::default()
    ));
    // With the selection on it: reverse video over the name and the fill.
    let lines = drawn(&looks, (None, Some(3)), (20, 10));
    assert_eq!(marks(&lines[4]), format!("...{}.", "u".repeat(16)));
    for span in lines[4]
        .spans
        .iter()
        .filter(|s| s.style.underline != Default::default())
    {
        assert!(span.style.reverse, "{span:?}");
    }
}

#[test]
fn a_plain_bar_underlines_the_file_as_well_and_keeps_the_reverse_selection() {
    let page = Style::default();
    let mark = Style {
        bold: true,
        ..Style::default()
    };
    let looks = Looks::plain(page, Style::default(), mark);
    let lines = drawn(&looks, (None, None), (20, 10));
    // No border: the last column is blank and is the mouse's.
    assert_eq!(marks(&lines[0]), format!("...{}.", "u".repeat(16)));
    assert_eq!(marks(&lines[1]), ".".repeat(20));
    let lines = drawn(&looks, (None, Some(3)), (20, 10));
    assert_eq!(marks(&lines[4]), format!("...{}.", "u".repeat(16)));
    for span in lines[4]
        .spans
        .iter()
        .filter(|s| s.style.underline != Default::default())
    {
        assert!(span.style.reverse, "{span:?}");
    }
}
