//! What a frame of the pager shows, as data: a `Scene` made of surfaces,
//! bottom first, and `flatten`, which lays the lines of a scene on the rows
//! of the window and tells how many rows each line takes. A scene holds no
//! bytes and does no I/O. `presenter.rs` writes a scene to a terminal, and
//! kitty's own sequences live in `kitty/`. A new kind of surface is added
//! here; its writing goes in the presenter.

use std::ops::Range;

use crate::typeset::size::TextSize;
use crate::typeset::style::{Line, Span};

/// What a frame shows, in the window's cells.
#[derive(Debug)]
pub struct Scene<'a> {
    /// The window in cells: columns, rows.
    pub window: (u16, u16),
    /// Bottom first. The `Lines` come first, each in an area of its own;
    /// the areas do not overlap. A bar may lie over several areas, or on a row
    /// that no area has; it is then written in every frame. It is not erased
    /// when it goes, so whoever draws on such a row erases it.
    pub surfaces: Vec<Surface<'a>>,
}

/// One layer of a scene.
#[derive(Debug)]
pub enum Surface<'a> {
    /// Lines from the top row of `area` down; dimmed behind a panel. Several
    /// lie side by side, each in its own area, and each is written at its
    /// area's column and ended at its area's right edge.
    Lines {
        area: Area,
        lines: &'a [Line],
        dim: bool,
    },
    /// Spans in their own styles on `row` from `column`, both from 1.
    Bar {
        row: usize,
        column: usize,
        spans: &'a [Span],
    },
    /// Areas of the window the pictures in them are darkened in, where
    /// the terminal can: behind a panel, which stays clear.
    Shade { areas: &'a [Area] },
}

/// A rectangle of the window, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    /// The column and the row of the top left cell, from 1.
    pub column: usize,
    pub row: usize,
    /// The cells across and down.
    pub columns: usize,
    pub rows: usize,
}

impl Area {
    /// The whole of a window of `window` columns and rows.
    pub fn window(window: (u16, u16)) -> Self {
        Self {
            column: 1,
            row: 1,
            columns: usize::from(window.0),
            rows: usize::from(window.1),
        }
    }

    /// Whether the area reaches the window's right edge.
    pub fn reaches_edge(&self, window: (u16, u16)) -> bool {
        self.column - 1 + self.columns >= usize::from(window.0)
    }

    /// Whether the area covers every column of the window, so that a row of
    /// it is the whole row of the screen.
    pub fn is_full_width(&self, window: (u16, u16)) -> bool {
        self.column == 1 && self.reaches_edge(window)
    }

    /// Whether the cell at `row` and `column`, from 1, is in the area.
    pub fn holds(&self, row: usize, column: usize) -> bool {
        (self.row..self.row + self.rows).contains(&row)
            && (self.column..self.column + self.columns).contains(&column)
    }
}

/// The window's areas of lines, each with a row for every row of it.
#[derive(Debug)]
pub struct Flat<'a> {
    /// In the order of the scene. A scene without lines has one area, the
    /// whole window, of rows with no line.
    pub areas: Vec<FlatArea<'a>>,
}

/// One area of lines.
#[derive(Debug)]
pub struct FlatArea<'a> {
    /// Where it is, cut at the window's edges.
    pub area: Area,
    /// One for each row of the area, from the top.
    pub rows: Vec<FlatRow<'a>>,
}

/// One row of an area.
#[derive(Debug)]
pub struct FlatRow<'a> {
    /// The rows of the area, from 0, of the line over this row; its own
    /// row alone where no line is.
    pub slot: Range<usize>,
    /// The line and which of its rows this row shows.
    pub line: Option<(&'a Line, u8)>,
    pub dim: bool,
    /// Whether the line spans more than one row with scaled text.
    pub scaled: bool,
}

impl<'a> Flat<'a> {
    /// Every row of every area, as the area's index, the row's index in it
    /// from 0 and the row, in the order of the areas.
    pub fn rows(&self) -> impl Iterator<Item = (usize, usize, &FlatRow<'a>)> {
        (self.areas.iter().enumerate())
            .flat_map(|(a, flat)| (flat.rows.iter().enumerate()).map(move |(r, row)| (a, r, row)))
    }
}

/// `scene`'s lines on the rows of their areas, `rows` telling how many rows
/// a line takes.
pub fn flatten<'a>(scene: &Scene<'a>, rows: impl Fn(&Line) -> u8) -> Flat<'a> {
    let window = Area::window(scene.window);
    let blank = |height: usize| -> Vec<FlatRow<'a>> {
        (0..height)
            .map(|r| FlatRow {
                slot: r..r + 1,
                line: None,
                dim: false,
                scaled: false,
            })
            .collect()
    };
    let mut areas = Vec::new();
    for surface in &scene.surfaces {
        let Surface::Lines { area, lines, dim } = surface else {
            continue;
        };
        let lines: &'a [Line] = lines;
        // Cut at the window, so that a row is never past its bottom.
        let area = Area {
            columns: area
                .columns
                .min((window.columns + 1).saturating_sub(area.column)),
            rows: area.rows.min((window.rows + 1).saturating_sub(area.row)),
            ..*area
        };
        let height = area.rows;
        let mut flat = blank(height);
        let mut top = 0_usize;
        for line in lines {
            let n = rows(line).max(1);
            if top + usize::from(n) > height {
                break;
            }
            let scaled = n > 1
                && line
                    .spans
                    .iter()
                    .any(|span| span.style.size != TextSize::NORMAL);
            for i in 0..n {
                flat[top + usize::from(i)] = FlatRow {
                    slot: top..top + usize::from(n),
                    line: Some((line, i)),
                    dim: *dim,
                    scaled,
                };
            }
            top += usize::from(n);
        }
        areas.push(FlatArea { area, rows: flat });
    }
    if areas.is_empty() {
        areas.push(FlatArea {
            area: window,
            rows: blank(window.rows),
        });
    }
    Flat { areas }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::style::Style;

    /// A line of one span, `text`, at `scale` (1 is the normal size).
    fn line(text: &str, scale: u8) -> Line {
        let size = TextSize::new(scale).unwrap_or(TextSize::NORMAL);
        Line::new(vec![Span::new(
            text,
            Style {
                size,
                ..Style::default()
            },
        )])
    }

    fn lines_scene(lines: &[Line], rows: u16, dim: bool) -> Scene<'_> {
        Scene {
            window: (40, rows),
            surfaces: vec![Surface::Lines {
                area: Area::window((40, rows)),
                lines,
                dim,
            }],
        }
    }

    #[test]
    fn each_line_fills_a_slot_of_its_rows_from_the_top() {
        let lines = [line("Big", 3), line("one", 1)];
        let flat = flatten(&lines_scene(&lines, 6, false), Line::rows);
        assert_eq!(flat.areas[0].rows.len(), 6);
        for r in 0..3 {
            assert_eq!(flat.areas[0].rows[r].slot, 0..3);
            assert_eq!(flat.areas[0].rows[r].line.map(|(_, at)| at), Some(r as u8));
            assert!(flat.areas[0].rows[r].scaled);
        }
        assert_eq!(flat.areas[0].rows[3].slot, 3..4);
        assert!(flat.areas[0].rows[3].line.is_some() && !flat.areas[0].rows[3].scaled);
        assert_eq!(
            (
                flat.areas[0].rows[4].slot.clone(),
                flat.areas[0].rows[4].line.is_none()
            ),
            (4..5, true)
        );
    }

    #[test]
    fn a_line_that_does_not_fit_whole_is_left_out_with_those_after_it() {
        let lines = [line("one", 1), line("Big", 3), line("two", 1)];
        let flat = flatten(&lines_scene(&lines, 3, false), Line::rows);
        assert!(flat.areas[0].rows[0].line.is_some());
        assert!(flat.areas[0].rows[1].line.is_none() && flat.areas[0].rows[2].line.is_none());
        assert_eq!(
            (
                flat.areas[0].rows[1].slot.clone(),
                flat.areas[0].rows[2].slot.clone()
            ),
            (1..2, 2..3)
        );
    }

    #[test]
    fn the_rows_of_the_lines_are_dimmed_as_their_surface_says() {
        let lines = [line("one", 1)];
        let scene = Scene {
            window: (40, 2),
            surfaces: vec![Surface::Lines {
                area: Area::window((40, 2)),
                lines: &lines,
                dim: true,
            }],
        };
        let flat = flatten(&scene, Line::rows);
        assert!(flat.areas[0].rows[0].dim);
        assert!(!flat.areas[0].rows[1].dim);
    }

    #[test]
    fn a_tall_line_without_scaled_text_is_one_slot_but_not_scaled() {
        let lines = [line("tall", 1)];
        let flat = flatten(&lines_scene(&lines, 4, false), |_| 2);
        for r in 0..2 {
            assert_eq!(flat.areas[0].rows[r].slot, 0..2);
            assert!(flat.areas[0].rows[r].line.is_some());
            assert!(!flat.areas[0].rows[r].scaled);
        }
    }

    #[test]
    fn a_scaled_line_on_one_row_is_not_scaled() {
        let lines = [line("Big", 3)];
        let flat = flatten(&lines_scene(&lines, 2, false), |_| 1);
        assert_eq!(flat.areas[0].rows[0].slot, 0..1);
        assert!(flat.areas[0].rows[0].line.is_some());
        assert!(!flat.areas[0].rows[0].scaled);
    }

    #[test]
    fn each_area_has_rows_of_its_own_and_the_slots_count_from_its_top() {
        let side = [line("a", 1), line("b", 1)];
        let doc = [line("Big", 2), line("c", 1)];
        let scene = Scene {
            window: (30, 4),
            surfaces: vec![
                Surface::Lines {
                    area: Area {
                        column: 1,
                        row: 1,
                        columns: 10,
                        rows: 4,
                    },
                    lines: &side,
                    dim: false,
                },
                Surface::Lines {
                    area: Area {
                        column: 11,
                        row: 2,
                        columns: 20,
                        rows: 3,
                    },
                    lines: &doc,
                    dim: true,
                },
            ],
        };
        let flat = flatten(&scene, Line::rows);
        assert_eq!(flat.areas.len(), 2);
        let (side, doc) = (&flat.areas[0], &flat.areas[1]);
        // The sidebar's lines are one row each and do not know the heading.
        assert_eq!((side.rows.len(), doc.rows.len()), (4, 3));
        assert_eq!(side.rows[1].slot, 1..2);
        assert!(side.rows[2].line.is_none());
        // The heading's slot is the document's first two rows, which are
        // the window's rows 2 and 3.
        assert_eq!(
            (doc.rows[0].slot.clone(), doc.rows[1].slot.clone()),
            (0..2, 0..2)
        );
        assert_eq!(doc.rows[2].slot, 2..3);
        assert!(doc.rows[0].scaled && doc.rows[0].dim && !side.rows[0].dim);
        assert_eq!(flat.rows().count(), 7);
    }

    #[test]
    fn an_area_is_cut_at_the_window_and_a_line_must_fit_in_it() {
        let lines = [line("one", 1), line("Big", 2)];
        let scene = Scene {
            window: (10, 3),
            surfaces: vec![Surface::Lines {
                area: Area {
                    column: 5,
                    row: 2,
                    columns: 20,
                    rows: 9,
                },
                lines: &lines,
                dim: false,
            }],
        };
        let flat = flatten(&scene, Line::rows);
        let area = flat.areas[0].area;
        assert_eq!((area.columns, area.rows), (6, 2));
        // The heading does not fit the two rows after the first line.
        assert!(flat.areas[0].rows[0].line.is_some());
        assert!(flat.areas[0].rows[1].line.is_none());
    }

    #[test]
    fn a_scene_without_lines_is_one_area_of_the_window_with_blank_rows() {
        let scene = Scene {
            window: (10, 3),
            surfaces: Vec::new(),
        };
        let flat = flatten(&scene, Line::rows);
        assert_eq!(flat.areas.len(), 1);
        assert_eq!(flat.areas[0].area, Area::window((10, 3)));
        assert!(flat.rows().all(|(_, _, row)| row.line.is_none()));
    }

    #[test]
    fn an_area_knows_whether_it_reaches_the_windows_edge() {
        let window = (30, 4);
        let side = Area {
            column: 1,
            row: 1,
            columns: 10,
            rows: 4,
        };
        let doc = Area {
            column: 11,
            columns: 20,
            ..side
        };
        assert!(!side.reaches_edge(window) && !side.is_full_width(window));
        assert!(doc.reaches_edge(window) && !doc.is_full_width(window));
        assert!(Area::window(window).is_full_width(window));
        assert!(doc.holds(4, 30) && !doc.holds(5, 30) && !doc.holds(1, 10));
    }
}
