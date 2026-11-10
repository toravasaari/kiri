//! Rows of cells, as a table or a front matter's map has them, built from the
//! document model for both layouts. A layout chooses what a cell's leaf holds.
//!
//! Its code uses neither layout engine; only its tests run both layouts' width
//! sharing, on one table of cases.

use crate::typeset::model::{Alignment, Inline, Table, Value};

/// What a cell holds: a leaf of the layout's own type, a grid of its own, or
/// cells one below another.
#[derive(Debug, Clone)]
pub enum Cell<T> {
    Leaf(T),
    /// A table of its own, as front matter shows a value that has keys.
    Table(Grid<T>),
    /// Cells one below another.
    Stack(Vec<Cell<T>>),
}

/// Rows of cells, as a table or a front matter's map has them.
#[derive(Debug, Clone)]
pub struct Grid<T> {
    pub rows: Vec<Vec<Cell<T>>>,
    pub alignments: Vec<Alignment>,
    /// Whether the first row is a header.
    pub header_row: bool,
    /// Whether the first column is a header.
    pub header_column: bool,
}

impl<T> Grid<T> {
    /// How many columns the longest row has. It counts the rows, not the
    /// alignments: front matter has none.
    pub fn columns(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// The rows below the header row, all of them when there is none.
    pub fn body(&self) -> &[Vec<Cell<T>>] {
        match self.header_row {
            true => self.rows.get(1..).unwrap_or_default(),
            false => &self.rows,
        }
    }
}

/// The grid of `table`, its header as the first row. `cell` makes a leaf
/// from a cell's inlines, and whether the cell is in the header.
pub fn table_grid<T>(table: &Table, cell: impl Fn(&[Inline], bool) -> T) -> Grid<T> {
    let row = |cells: &Vec<Vec<Inline>>, header| {
        (cells.iter())
            .map(|inlines| Cell::Leaf(cell(inlines, header)))
            .collect()
    };
    Grid {
        rows: std::iter::once(row(&table.header, true))
            .chain(table.rows.iter().map(|cells| row(cells, false)))
            .collect(),
        alignments: table.alignments.clone(),
        header_row: true,
        header_column: false,
    }
}

/// What a layout's leaves are made of in front matter's grid.
pub trait Leaves<T> {
    /// A key, in the header column.
    fn key(&self, key: &str) -> T;
    /// A scalar, or a list of scalars joined with commas.
    fn text(&self, text: &str) -> T;
    /// An empty map.
    fn empty(&self) -> T;
    /// A value nested deeper than tables go, as YAML.
    fn yaml(&self, yaml: &str) -> T;
}

/// `entries` as a grid at `level`, its keys in a header column.
pub fn front_matter_grid<T>(
    entries: &[(String, Value)],
    level: usize,
    leaves: &impl Leaves<T>,
) -> Grid<T> {
    Grid {
        rows: (entries.iter())
            .map(|(key, value)| {
                vec![
                    Cell::Leaf(leaves.key(key)),
                    value_cell(value, level, leaves),
                ]
            })
            .collect(),
        alignments: Vec::new(),
        header_row: false,
        header_column: true,
    }
}

/// `value` in a cell of a grid at `level`.
fn value_cell<T>(value: &Value, level: usize, leaves: &impl Leaves<T>) -> Cell<T> {
    match value {
        Value::Map(entries) if entries.is_empty() => Cell::Leaf(leaves.empty()),
        Value::List(items) if items.iter().all(Value::is_scalar) => {
            let items: Vec<&str> = items.iter().filter_map(Value::scalar).collect();
            Cell::Leaf(leaves.text(&items.join(", ")))
        }
        Value::Map(entries) if level < Value::TABLE_LEVELS => {
            Cell::Table(front_matter_grid(entries, level + 1, leaves))
        }
        Value::List(items) if level < Value::TABLE_LEVELS => Cell::Stack(
            (items.iter())
                .map(|item| match item {
                    Value::Map(entries) if !entries.is_empty() => {
                        Cell::Table(front_matter_grid(entries, level + 1, leaves))
                    }
                    item => value_cell(item, level, leaves),
                })
                .collect(),
        ),
        Value::Map(_) | Value::List(_) => Cell::Leaf(leaves.yaml(&value.yaml())),
        scalar => Cell::Leaf(leaves.text(scalar.scalar().unwrap_or_default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::layout::table::shrink;
    use crate::typeset::reading::page::table::share;

    fn text(text: &str) -> Vec<Inline> {
        vec![Inline::Text(text.to_owned())]
    }

    fn table() -> Table {
        Table {
            alignments: vec![Alignment::Left, Alignment::Right],
            header: vec![text("a"), text("b")],
            rows: vec![vec![text("1"), text("2")], vec![text("3")]],
        }
    }

    #[test]
    fn a_table_has_its_header_as_the_first_row() {
        let grid = table_grid(&table(), |_, header| header);
        assert!(grid.header_row && !grid.header_column);
        assert_eq!(grid.rows.len(), 3);
        let flags: Vec<Vec<bool>> = (grid.rows.iter())
            .map(|row| {
                (row.iter())
                    .map(|cell| matches!(cell, Cell::Leaf(true)))
                    .collect()
            })
            .collect();
        assert_eq!(
            flags,
            vec![vec![true, true], vec![false, false], vec![false]]
        );
        assert_eq!(grid.alignments, [Alignment::Left, Alignment::Right]);
    }

    #[test]
    fn columns_come_from_the_longest_row_and_the_body_leaves_out_the_header() {
        let grid = table_grid(&table(), |inlines, _| inlines.len());
        assert_eq!(grid.columns(), 2);
        assert_eq!(grid.body().len(), 2);
        let empty = Grid::<()> {
            rows: Vec::new(),
            alignments: Vec::new(),
            header_row: true,
            header_column: false,
        };
        assert_eq!((empty.columns(), empty.body().len()), (0, 0));
    }

    // The least widths, the most, the room, and the widths both give.
    type Case = (
        &'static [usize],
        &'static [usize],
        usize,
        Option<&'static [usize]>,
    );

    /// Cases on which the cell layout's `shrink` and the pixel layout's
    /// `share` must agree, with one cell or one pixel to a unit: the least
    /// and most width of each column, and the room. The last value is the
    /// widths both give, or `None` where the least widths do not fit, when
    /// only whether they fit is compared.
    const SHARED_CASES: &[Case] = &[
        // The content fits, so every column takes its most.
        (&[10, 20], &[30, 40], 100, Some(&[30, 40])),
        // The spare room goes to the columns that can grow, in proportion.
        (&[10, 20], &[30, 60], 60, Some(&[20, 40])),
        (&[10, 10, 10], &[20, 30, 40], 60, Some(&[15, 20, 25])),
        // A column at its least takes none of the spare room.
        (&[10, 20], &[10, 60], 40, Some(&[10, 30])),
        // The least widths use up all the room.
        (&[30, 40], &[50, 60], 70, Some(&[30, 40])),
        // The least widths do not fit.
        (&[30, 40], &[50, 60], 60, None),
    ];

    #[test]
    fn the_cell_and_pixel_sharings_agree_on_the_shared_cases() {
        let pixels = |widths: &[usize]| widths.iter().map(|&w| w as f32).collect::<Vec<_>>();
        for &(least, most, room, expected) in SHARED_CASES {
            let mut cells = most.to_vec();
            let cells_fit = shrink(&mut cells, least, room);
            let (shared, breaks) = share(&pixels(least), &pixels(most), room as f32);
            assert_eq!(cells_fit, !breaks, "fit for {least:?} {most:?} {room}");
            if let Some(expected) = expected {
                assert_eq!(cells, expected, "cells for {least:?} {most:?} {room}");
                assert_eq!(shared, pixels(expected), "pixels for {least:?} {most:?}");
            }
        }
    }
}
