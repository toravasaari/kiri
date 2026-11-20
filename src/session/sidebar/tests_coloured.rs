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
        &looks(),
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
        &Looks::coloured(&colors, Style::default()),
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
            &looks(),
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
        &looks,
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
        &looks,
    );
    assert!(span(&lines[1], "Two").style.reverse);
    // The entry the reader is in is quiet, not reversed, and no edge.
    assert!(!span(&lines[2], "Three").style.reverse);
    assert!(span(&lines[2], "Three").style.italic);
}
