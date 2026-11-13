//! Rows of cells, as a table or a front matter's map has them, built from the
//! document model for both layouts. A layout chooses what a cell's leaf holds.
//!
//! Its code uses neither layout engine; only its tests run both layouts' width
//! sharing, on one table of cases.

use std::cell::Cell as Counter;

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

/// The grid of `table`, its header as the first row. `cell` makes a leaf from
/// the number of the cell among the texts of the table, which the grid counts
/// a row at a time, from a cell's inlines, and whether the cell is in the
/// header.
pub fn table_grid<T>(table: &Table, cell: impl Fn(usize, &[Inline], bool) -> T) -> Grid<T> {
    let next = Counter::new(0);
    let row = |cells: &Vec<Vec<Inline>>, header| {
        (cells.iter())
            .map(|inlines| Cell::Leaf(cell(next.replace(next.get() + 1), inlines, header)))
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

/// What a layout's leaves are made of in front matter's grid. Each takes its
/// number among the texts of the front matter, which the grid counts: a key,
/// then its value, and what is in the value before the next key.
pub trait Leaves<T> {
    /// A key, in the header column.
    fn key(&self, number: usize, key: &str) -> T;
    /// A scalar, or a list of scalars joined with commas.
    fn text(&self, number: usize, text: &str) -> T;
    /// An empty map.
    fn empty(&self, number: usize) -> T;
    /// A value nested deeper than tables go, as YAML.
    fn yaml(&self, number: usize, yaml: &str) -> T;
}

/// Front matter's `entries` as a grid, its keys in a header column.
pub fn front_matter_grid<T>(entries: &[(String, Value)], leaves: &impl Leaves<T>) -> Grid<T> {
    map_grid(entries, 1, &Counter::new(0), leaves)
}

/// `entries` as a grid at `level`, its keys in a header column. `next` is the
/// number of the next leaf.
fn map_grid<T>(
    entries: &[(String, Value)],
    level: usize,
    next: &Counter<usize>,
    leaves: &impl Leaves<T>,
) -> Grid<T> {
    let number = || next.replace(next.get() + 1);
    Grid {
        rows: (entries.iter())
            .map(|(key, value)| {
                let key = Cell::Leaf(leaves.key(number(), key));
                vec![key, value_cell(value, level, next, leaves)]
            })
            .collect(),
        alignments: Vec::new(),
        header_row: false,
        header_column: true,
    }
}

/// `value` in a cell of a grid at `level`.
fn value_cell<T>(
    value: &Value,
    level: usize,
    next: &Counter<usize>,
    leaves: &impl Leaves<T>,
) -> Cell<T> {
    let number = || next.replace(next.get() + 1);
    match value {
        Value::Map(entries) if entries.is_empty() => Cell::Leaf(leaves.empty(number())),
        Value::List(items) if items.iter().all(Value::is_scalar) => {
            let items: Vec<&str> = items.iter().filter_map(Value::scalar).collect();
            Cell::Leaf(leaves.text(number(), &items.join(", ")))
        }
        Value::Map(entries) if level < Value::TABLE_LEVELS => {
            Cell::Table(map_grid(entries, level + 1, next, leaves))
        }
        Value::List(items) if level < Value::TABLE_LEVELS => Cell::Stack(
            (items.iter())
                .map(|item| match item {
                    Value::Map(entries) if !entries.is_empty() => {
                        Cell::Table(map_grid(entries, level + 1, next, leaves))
                    }
                    item => value_cell(item, level, next, leaves),
                })
                .collect(),
        ),
        Value::Map(_) | Value::List(_) => Cell::Leaf(leaves.yaml(number(), &value.yaml())),
        scalar => Cell::Leaf(leaves.text(number(), scalar.scalar().unwrap_or_default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::layout::table::shrink;
    use crate::typeset::model::Block;
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
        let grid = table_grid(&table(), |_, _, header| header);
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
        let grid = table_grid(&table(), |_, inlines, _| inlines.len());
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

    /// Leaves that are their own numbers.
    struct Numbers;

    impl Leaves<usize> for Numbers {
        fn key(&self, number: usize, _: &str) -> usize {
            number
        }
        fn text(&self, number: usize, _: &str) -> usize {
            number
        }
        fn empty(&self, number: usize) -> usize {
            number
        }
        fn yaml(&self, number: usize, _: &str) -> usize {
            number
        }
    }

    /// The numbers of the leaves of `cell`, in the order it holds them.
    fn numbers(cell: &Cell<usize>, out: &mut Vec<usize>) {
        match cell {
            Cell::Leaf(number) => out.push(*number),
            Cell::Table(grid) => grid.rows.iter().flatten().for_each(|c| numbers(c, out)),
            Cell::Stack(cells) => cells.iter().for_each(|c| numbers(c, out)),
        }
    }

    #[test]
    fn the_cells_of_a_table_are_numbered_a_row_at_a_time_from_the_header() {
        let grid = table_grid(&table(), |number, _, _| number);
        let mut all = Vec::new();
        grid.rows
            .iter()
            .flatten()
            .for_each(|c| numbers(c, &mut all));
        assert_eq!(all, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn the_leaves_of_front_matter_are_numbered_a_key_and_then_its_value_and_what_is_in_it() {
        let yaml = "title: A\nmeta:\n  year: 1\n  deep:\n    more: 2\nstops:\n  - name: B\n    km: 3\n  - plain\ntags: [a, b]\nnone: {}\n";
        let Block::FrontMatter(front_matter) =
            &crate::typeset::parse::parse(&format!("---\n{yaml}---\n")).blocks[0].clone()
        else {
            panic!("front matter")
        };
        let Some(Ok(Value::Map(entries))) = &front_matter.value else {
            panic!("a map")
        };
        let grid = front_matter_grid(entries, &Numbers);
        let mut all = Vec::new();
        grid.rows
            .iter()
            .flatten()
            .for_each(|c| numbers(c, &mut all));
        assert_eq!(all, (0..all.len()).collect::<Vec<_>>());
        assert_eq!(all.len(), 18);
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
