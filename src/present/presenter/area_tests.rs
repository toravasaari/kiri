//! Areas side by side: where each is written, how its rows are erased and
//! ended, and what stays alone when only one of them changes.

use super::*;
use crate::present::encoder::Encoder;
use crate::present::output::Page;
use crate::typeset::caps::Capabilities;
use crate::typeset::color::Color;
use crate::typeset::images::Images;
use crate::typeset::size::TextSize;
use crate::typeset::style::{Span, Style};

const WINDOW: (u16, u16) = (30, 4);

/// The sidebar: the left 10 columns of the window.
const SIDE: Area = Area {
    column: 1,
    row: 1,
    columns: 10,
    rows: 4,
};

/// The document: the 20 columns right of the sidebar, to the window's edge.
const DOC: Area = Area {
    column: 11,
    row: 1,
    columns: 20,
    rows: 4,
};

/// The document on the left and the sidebar on the right.
const DOC_LEFT: Area = Area {
    column: 1,
    row: 1,
    columns: 20,
    rows: 4,
};
const SIDE_RIGHT: Area = Area {
    column: 21,
    row: 1,
    columns: 10,
    rows: 4,
};

fn text(text: &str) -> Line {
    Line::new(vec![Span::new(text, Style::default())])
}

fn heading(text: &str) -> Line {
    let size = TextSize::new(2).unwrap();
    Line::new(vec![Span::new(
        text,
        Style {
            size,
            ..Style::default()
        },
    )])
}

/// What presenting each frame writes, one after the other through one
/// presenter. A frame is a list of `(area, lines)`.
fn presented(caps: &Capabilities, page: Page, frames: &[&[(Area, &[Line])]]) -> Vec<String> {
    let images = Images::new();
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, caps, page, &mut encoder);
    let mut presenter = Presenter::default();
    frames
        .iter()
        .map(|areas| {
            let scene = Scene {
                window: WINDOW,
                surfaces: (areas.iter())
                    .map(|&(area, lines)| Surface::Lines {
                        area,
                        lines,
                        dim: false,
                    })
                    .collect(),
            };
            let mut out = Vec::new();
            presenter.present(&scene, &mut writer, &mut out).unwrap();
            String::from_utf8(out).unwrap()
        })
        .collect()
}

fn kitty() -> Capabilities {
    Capabilities::kitty(WINDOW.0)
}

fn page() -> Page {
    Page {
        background: Some(Color::Rgb(1, 2, 3)),
        foreground: None,
    }
}

#[test]
fn each_area_is_written_from_its_own_column() {
    let (side, doc) = ([text("side"), text("two")], [text("doc"), text("more")]);
    let frames = presented(&kitty(), Page::default(), &[&[(SIDE, &side), (DOC, &doc)]]);
    let out = &frames[0];
    for written in [
        "\x1b[1;1Hside",
        "\x1b[2;1Htwo",
        "\x1b[1;11Hdoc",
        "\x1b[2;11Hmore",
    ] {
        assert!(out.contains(written), "{written:?} in {out:?}");
    }
    // Nothing of the document starts at the window's first column.
    assert!(!out.contains("\x1b[1;1Hdoc"), "{out:?}");
}

#[test]
fn a_row_is_dirty_in_its_area_alone() {
    let before = ([text("a"), text("b")], [text("x"), text("y")]);
    let after = ([text("a"), text("B")], [text("x"), text("y")]);
    let frames = presented(
        &kitty(),
        Page::default(),
        &[
            &[(SIDE, &before.0), (DOC, &before.1)],
            &[(SIDE, &after.0), (DOC, &after.1)],
        ],
    );
    // Only row 2 of the sidebar: erased in its columns, then written. The
    // document's rows, and the sidebar's first, are left alone.
    assert_eq!(
        frames[1],
        "\x1b[?2026h\x1b[2;1H\x1b[10X\x1b[2;1HB\x1b[?2026l"
    );
}

#[test]
fn a_change_in_the_document_leaves_the_sidebar_alone() {
    let side = [text("a"), text("b")];
    let (before, after) = ([text("x"), text("y")], [text("x"), text("Y")]);
    let frames = presented(
        &kitty(),
        Page::default(),
        &[
            &[(SIDE, &side), (DOC, &before)],
            &[(SIDE, &side), (DOC, &after)],
        ],
    );
    assert_eq!(
        frames[1],
        "\x1b[?2026h\x1b[2;11H\x1b[20X\x1b[2;11HY\x1b[?2026l"
    );
}

#[test]
fn a_row_of_an_area_that_is_not_the_whole_window_is_erased_with_ech_never_2k() {
    let (before, after) = ([text("a")], [text("b")]);
    for (area, erase) in [
        (SIDE, "\x1b[1;1H\x1b[10X"),
        (DOC, "\x1b[1;11H\x1b[20X"),
        (SIDE_RIGHT, "\x1b[1;21H\x1b[10X"),
    ] {
        let frames = presented(
            &kitty(),
            Page::default(),
            &[&[(area, &before)], &[(area, &after)]],
        );
        assert!(frames[1].contains(erase), "{:?}", frames[1]);
        assert!(!frames[1].contains("\x1b[2K"), "{:?}", frames[1]);
    }
}

#[test]
fn a_row_of_an_area_over_the_whole_width_is_still_erased_with_2k() {
    let whole = Area::window(WINDOW);
    let (before, after) = ([text("a")], [text("b")]);
    let frames = presented(
        &kitty(),
        Page::default(),
        &[&[(whole, &before)], &[(whole, &after)]],
    );
    assert_eq!(
        frames[1],
        "\x1b[?2026h\x1b[1;1H\x1b[2K\x1b[1;1Hb\x1b[?2026l"
    );
}

#[test]
fn a_row_ends_with_a_clear_to_the_edge_only_where_its_area_reaches_the_window_edge() {
    let (side, doc) = ([text("ab")], [text("cd")]);
    let frames = presented(&kitty(), page(), &[&[(SIDE, &side), (DOC, &doc)]]);
    let out = &frames[0];
    // The sidebar's row has 8 cells left in its area, which are erased; the
    // document's row reaches the window's edge and is cleared to it.
    assert!(
        out.contains("\x1b[1;1H\x1b[0;48;2;1;2;3mab\x1b[0m\x1b[0;48;2;1;2;3m\x1b[8X\x1b[0m"),
        "{out:?}"
    );
    assert!(out.contains("cd\x1b[K"), "{out:?}");
    // The sidebar's row has no clear to the edge, which would take the
    // document's cells.
    let side_row = out.split("\x1b[1;11H").next().unwrap();
    assert!(!side_row.contains("\x1b[K"), "{side_row:?}");
    // On the right of the document the roles are the other way round.
    let frames = presented(
        &kitty(),
        page(),
        &[&[(DOC_LEFT, &doc), (SIDE_RIGHT, &side)]],
    );
    let out = &frames[0];
    assert!(
        out.contains("cd\x1b[0m\x1b[0;48;2;1;2;3m\x1b[18X"),
        "{out:?}"
    );
    assert!(out.contains("ab\x1b[K"), "{out:?}");
}

#[test]
fn the_erase_of_an_area_beside_a_two_row_heading_leaves_the_heading_alone() {
    // A heading of two rows on the left, a sidebar of one-row lines on the
    // right. Its second row changes, which is the heading's lower row: the
    // erase covers the sidebar's columns, 21 to 30, and nothing left of
    // them, where the heading's lower cells are.
    let doc = [heading("Big")];
    let (before, after) = ([text("a"), text("b")], [text("a"), text("c")]);
    let frames = presented(
        &kitty(),
        Page::default(),
        &[
            &[(DOC_LEFT, &doc), (SIDE_RIGHT, &before)],
            &[(DOC_LEFT, &doc), (SIDE_RIGHT, &after)],
        ],
    );
    assert_eq!(
        frames[1],
        "\x1b[?2026h\x1b[2;21H\x1b[10X\x1b[2;21Hc\x1b[?2026l"
    );
}

#[test]
fn a_heading_in_an_area_has_its_lower_row_filled_up_to_the_area_edge() {
    let doc = [heading("Big")];
    let frames = presented(&kitty(), page(), &[&[(DOC_LEFT, &doc), (SIDE_RIGHT, &[])]]);
    let out = &frames[0];
    // The lower row, written at the document's column, steps over the
    // heading's 6 cells and erases the 14 that the area has left.
    assert!(out.contains("\x1b[2;1H\x1b[6C"), "{out:?}");
    assert!(out.contains("\x1b[14X"), "{out:?}");
}

#[test]
fn areas_that_change_shape_are_drawn_whole() {
    let side = [text("a")];
    let doc = [text("x")];
    let frames = presented(
        &kitty(),
        Page::default(),
        &[
            &[(SIDE, &side), (DOC, &doc)],
            &[(SIDE, &side), (DOC, &doc)],
            &[
                (Area { columns: 8, ..SIDE }, &side),
                (
                    Area {
                        column: 9,
                        columns: 22,
                        ..DOC
                    },
                    &doc,
                ),
            ],
        ],
    );
    assert_eq!(frames[1], "\x1b[?2026h\x1b[?2026l");
    assert!(frames[2].contains("\x1b[2J"), "{:?}", frames[2]);
    assert!(frames[2].contains("\x1b[1;9Hx"), "{:?}", frames[2]);
}

#[test]
fn a_bar_over_an_area_is_in_the_rows_of_that_area() {
    let (side, doc) = ([text("a")], [text("b")]);
    let spans = [Span::new("S", Style::default())];
    let scene = |bar_row| Scene {
        window: WINDOW,
        surfaces: vec![
            Surface::Lines {
                area: SIDE,
                lines: &side,
                dim: false,
            },
            Surface::Lines {
                area: DOC,
                lines: &doc,
                dim: false,
            },
            Surface::Bar {
                row: bar_row,
                column: 12,
                spans: &spans,
            },
        ],
    };
    let images = Images::new();
    let caps = kitty();
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, &caps, Page::default(), &mut encoder);
    let mut presenter = Presenter::default();
    let mut write = |bar_row| {
        let mut out = Vec::new();
        presenter
            .present(&scene(bar_row), &mut writer, &mut out)
            .unwrap();
        String::from_utf8(out).unwrap()
    };
    write(2);
    // The bar moves from row 2 to row 3: both rows of the document's area
    // are written again, and the sidebar's are not.
    let out = write(3);
    assert_eq!(
        out,
        "\x1b[?2026h\x1b[2;11H\x1b[20X\x1b[3;11H\x1b[20X\x1b[3;12HS\x1b[?2026l"
    );
}

/// Presents `frames`, each a list of area lines and the bars `(row, column,
/// text)` over them, through one presenter in a window of 30 × 4.
type Areas<'a> = &'a [(Area, &'a [Line])];
type Bars<'a> = &'a [(usize, usize, &'a str)];

fn presented_with_bars(caps: &Capabilities, frames: &[(Areas<'_>, Bars<'_>)]) -> Vec<String> {
    let images = Images::new();
    let mut encoder = Encoder::new();
    let mut writer = Writer::new(&images, caps, Page::default(), &mut encoder);
    let mut presenter = Presenter::default();
    frames
        .iter()
        .map(|(areas, bars)| {
            let spans: Vec<[Span; 1]> = (bars.iter())
                .map(|(_, _, text)| [Span::new(text, Style::default())])
                .collect();
            let mut surfaces: Vec<Surface<'_>> = (areas.iter())
                .map(|&(area, lines)| Surface::Lines {
                    area,
                    lines,
                    dim: false,
                })
                .collect();
            surfaces.extend(
                bars.iter()
                    .zip(&spans)
                    .map(|(&(row, column, _), spans)| Surface::Bar { row, column, spans }),
            );
            let scene = Scene {
                window: WINDOW,
                surfaces,
            };
            let mut out = Vec::new();
            presenter.present(&scene, &mut writer, &mut out).unwrap();
            String::from_utf8(out).unwrap()
        })
        .collect()
}

#[test]
fn a_bar_across_two_areas_is_written_again_when_a_row_under_it_changes() {
    let side = [text("a"), text("b")];
    let (before, after) = ([text("x"), text("y")], [text("x"), text("Y")]);
    let bar = "0123456789012345678901234567890"; // 31 cells, cut by the window
    let bars: &[(usize, usize, &str)] = &[(2, 1, &bar[..30])];
    let frames = presented_with_bars(
        &kitty(),
        &[
            (&[(SIDE, &side), (DOC, &before)], bars),
            (&[(SIDE, &side), (DOC, &after)], bars),
        ],
    );
    // The document's row 2 is erased over the bar's cells in its columns, so
    // the bar goes on after the row.
    let out = &frames[1];
    let erase = out.find("\x1b[2;11H\x1b[20X").expect(out);
    let line = out.find("\x1b[2;11HY").expect(out);
    let bar = out.find("\x1b[2;1H0123456789").expect(out);
    assert!(erase < line && line < bar, "{out:?}");
}

#[test]
fn a_bar_across_two_areas_is_written_after_every_line_of_its_row_by_row_on_sixel() {
    let side = [text("a"), text("b")];
    let (before, after) = ([text("x"), text("y")], [text("x"), text("Y")]);
    let bars: &[(usize, usize, &str)] = &[(2, 1, "012345678901234567890123456789")];
    let frames = presented_with_bars(
        &Capabilities::sixel(30),
        &[
            (&[(SIDE, &side), (DOC, &before)], bars),
            (&[(SIDE, &side), (DOC, &after)], bars),
        ],
    );
    let out = &frames[1];
    assert!(
        out.find("\x1b[2;11HY").expect(out) < out.find("\x1b[2;1H0123").expect(out),
        "{out:?}"
    );
}

#[test]
fn a_bar_on_a_row_no_area_covers_is_written_in_every_frame() {
    let area = Area {
        rows: 3,
        ..Area::window(WINDOW)
    };
    let lines = [text("x")];
    let bars: &[(usize, usize, &str)] = &[(4, 1, "STATUS")];
    let frames = presented_with_bars(
        &kitty(),
        &[
            (&[(area, &lines)], bars),
            (&[(area, &lines)], bars),
            (&[(area, &lines)], &[]),
        ],
    );
    assert!(frames[0].contains("\x1b[4;1HSTATUS"), "{:?}", frames[0]);
    assert!(frames[1].contains("\x1b[4;1HSTATUS"), "{:?}", frames[1]);
    assert!(!frames[2].contains("STATUS"), "{:?}", frames[2]);
}
