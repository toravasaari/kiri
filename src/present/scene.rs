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
    /// Bottom first. At most one `Lines`, and it comes first.
    pub surfaces: Vec<Surface<'a>>,
}

/// One layer of a scene.
#[derive(Debug)]
pub enum Surface<'a> {
    /// The document's lines from the top row down; dimmed behind a panel.
    Lines { lines: &'a [Line], dim: bool },
    /// Spans in their own styles on `row` from `column`, both from 1.
    Bar {
        row: usize,
        column: usize,
        spans: &'a [Span],
    },
}

/// The window's rows, each with the line that covers it.
#[derive(Debug)]
pub struct Flat<'a> {
    /// One for each row of the window, from the top.
    pub rows: Vec<FlatRow<'a>>,
}

/// One row of the window.
#[derive(Debug)]
pub struct FlatRow<'a> {
    /// The rows, from 0, of the line over this row; its own row alone
    /// where no line is.
    pub slot: Range<usize>,
    /// The line and which of its rows this row shows.
    pub line: Option<(&'a Line, u8)>,
    pub dim: bool,
    /// Whether the line spans more than one row with scaled text.
    pub scaled: bool,
}

/// `scene`'s lines on the window's rows, `rows` telling how many rows a
/// line takes.
pub fn flatten<'a>(scene: &Scene<'a>, rows: impl Fn(&Line) -> u8) -> Flat<'a> {
    let height = usize::from(scene.window.1);
    let mut flat: Vec<FlatRow<'a>> = (0..height)
        .map(|r| FlatRow {
            slot: r..r + 1,
            line: None,
            dim: false,
            scaled: false,
        })
        .collect();
    for surface in &scene.surfaces {
        let Surface::Lines { lines, dim } = surface else {
            continue;
        };
        let lines: &'a [Line] = lines;
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
    }
    Flat { rows: flat }
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
            surfaces: vec![Surface::Lines { lines, dim }],
        }
    }

    #[test]
    fn each_line_fills_a_slot_of_its_rows_from_the_top() {
        let lines = [line("Big", 3), line("one", 1)];
        let flat = flatten(&lines_scene(&lines, 6, false), Line::rows);
        assert_eq!(flat.rows.len(), 6);
        for r in 0..3 {
            assert_eq!(flat.rows[r].slot, 0..3);
            assert_eq!(flat.rows[r].line.map(|(_, at)| at), Some(r as u8));
            assert!(flat.rows[r].scaled);
        }
        assert_eq!(flat.rows[3].slot, 3..4);
        assert!(flat.rows[3].line.is_some() && !flat.rows[3].scaled);
        assert_eq!(
            (flat.rows[4].slot.clone(), flat.rows[4].line.is_none()),
            (4..5, true)
        );
    }

    #[test]
    fn a_line_that_does_not_fit_whole_is_left_out_with_those_after_it() {
        let lines = [line("one", 1), line("Big", 3), line("two", 1)];
        let flat = flatten(&lines_scene(&lines, 3, false), Line::rows);
        assert!(flat.rows[0].line.is_some());
        assert!(flat.rows[1].line.is_none() && flat.rows[2].line.is_none());
        assert_eq!(
            (flat.rows[1].slot.clone(), flat.rows[2].slot.clone()),
            (1..2, 2..3)
        );
    }

    #[test]
    fn the_rows_of_the_lines_are_dimmed_as_their_surface_says() {
        let lines = [line("one", 1)];
        let scene = Scene {
            window: (40, 2),
            surfaces: vec![Surface::Lines {
                lines: &lines,
                dim: true,
            }],
        };
        let flat = flatten(&scene, Line::rows);
        assert!(flat.rows[0].dim);
        assert!(!flat.rows[1].dim);
    }

    #[test]
    fn a_tall_line_without_scaled_text_is_one_slot_but_not_scaled() {
        let lines = [line("tall", 1)];
        let flat = flatten(&lines_scene(&lines, 4, false), |_| 2);
        for r in 0..2 {
            assert_eq!(flat.rows[r].slot, 0..2);
            assert!(flat.rows[r].line.is_some());
            assert!(!flat.rows[r].scaled);
        }
    }

    #[test]
    fn a_scaled_line_on_one_row_is_not_scaled() {
        let lines = [line("Big", 3)];
        let flat = flatten(&lines_scene(&lines, 2, false), |_| 1);
        assert_eq!(flat.rows[0].slot, 0..1);
        assert!(flat.rows[0].line.is_some());
        assert!(!flat.rows[0].scaled);
    }
}
