//! Tables: how their columns share the room, and how their cells, nested
//! grids and the rows of a table are laid out.

use crate::typeset::grid;
use crate::typeset::model::{Alignment, Inline, Table};
use crate::typeset::reading::page::{Cell, FITS, Flow, Grid, Ink, Shape, TableStyle, Wide, runs};
use crate::typeset::reading::text::{self, Paragraph};

impl Flow<'_> {
    /// Lays a table out at `x`: as wide as its content up to the area's
    /// edge, with its columns shared out as a browser shares them.
    pub(super) fn table(&mut self, table: &Table, x: f32) {
        let grid = grid::table_grid(table, |inlines, _| inlines.to_vec());
        self.grid_at(&grid, x, self.area - x, self.wide == Wide::Scroll);
    }

    /// Lays `grid` out at `x` in at most `room` pixels, scrolling in a
    /// band when `scroll` is set and even its narrowest columns do not fit.
    fn grid_at(&mut self, grid: &Grid, x: f32, room: f32, scroll: bool) {
        let columns = grid.columns();
        if columns == 0 {
            return;
        }
        let ink = self.ink(room);
        let (least, most) = self.grid_widths(grid, ink);
        // Each column takes its padding and the border at its left; the
        // last border closes the table.
        let extra = self.look.size * 2.0 * self.look.spacing.cell_across + 1.0;
        let room = room - columns as f32 * extra - 1.0;
        let (widths, forced) = share(&least, &most, room);
        // Whether to scroll: even the least widths are too wide for the
        // area, by more than rounding, which `share` allows for.
        let too_wide = forced;
        let (widths, forced) = match too_wide && scroll {
            // Scrolling, words stay whole: each column takes its least
            // width, and more up to half the area, and the table scrolls.
            true => {
                let half = (self.area - x) / 2.0;
                let widths = (least.iter().zip(&most))
                    .map(|(min, max)| min.max(max.min(half)))
                    .collect();
                (widths, false)
            }
            false => (widths, forced),
        };
        let wrap = if forced {
            text::Wrap::Forced
        } else {
            text::Wrap::Words
        };
        let table_width = widths.iter().sum::<f32>() + columns as f32 * extra + 1.0;
        match scroll && too_wide {
            true => self.band(x, table_width, |flow| {
                flow.grid(grid, x, &widths, wrap);
                None
            }),
            false => self.grid(grid, x, &widths, wrap),
        }
    }

    /// The narrowest and the widest each column of `grid` can be.
    fn grid_widths(&self, grid: &Grid, ink: Ink) -> (Vec<f32>, Vec<f32>) {
        let columns = grid.columns();
        let (mut least, mut most) = (vec![0.0f32; columns], vec![0.0f32; columns]);
        for (row, cells) in grid.rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate() {
                let (min, max) = self.cell_widths(grid, (row, column), cell, ink);
                least[column] = least[column].max(min);
                most[column] = most[column].max(max);
            }
        }
        (least, most)
    }

    /// The narrowest and the widest `cell`, at (`row`, `column`) of `grid`,
    /// can be.
    fn cell_widths(
        &self,
        grid: &Grid,
        (row, column): (usize, usize),
        cell: &Cell,
        ink: Ink,
    ) -> (f32, f32) {
        match cell {
            Cell::Leaf(content) => self
                .cell_text(
                    grid,
                    (row, column),
                    content,
                    f32::MAX,
                    text::Wrap::Words,
                    ink,
                )
                .content_widths(),
            Cell::Table(inner) => {
                let columns = inner.columns();
                let (least, most) = self.grid_widths(inner, ink);
                let extra = self.look.size * 2.0 * self.look.spacing.cell_across + 1.0;
                let border = columns as f32 * extra + 1.0;
                (
                    least.iter().sum::<f32>() + border,
                    most.iter().sum::<f32>() + border,
                )
            }
            Cell::Stack(cells) => {
                // The tables of a stack share their columns' widths.
                let extra = self.look.size * 2.0 * self.look.spacing.cell_across + 1.0;
                let tables = self.stack_columns(cells, ink).map(|(least, most)| {
                    let border = least.len() as f32 * extra + 1.0;
                    (
                        least.iter().sum::<f32>() + border,
                        most.iter().sum::<f32>() + border,
                    )
                });
                let others = cells
                    .iter()
                    .filter(|cell| !matches!(cell, Cell::Table(_)))
                    .map(|cell| self.cell_widths(grid, (row, column), cell, ink));
                (others.chain(tables)).fold((0.0, 0.0), |(min, max), (least, most)| {
                    (f32::max(min, least), f32::max(max, most))
                })
            }
        }
    }

    /// The narrowest and the widest each column of the tables among
    /// `cells` can be, the widest of each column across them; `None` when
    /// there are none.
    fn stack_columns(&self, cells: &[Cell], ink: Ink) -> Option<(Vec<f32>, Vec<f32>)> {
        let mut found: Option<(Vec<f32>, Vec<f32>)> = None;
        for cell in cells {
            let Cell::Table(inner) = cell else {
                continue;
            };
            let (least, most) = self.grid_widths(inner, ink);
            let (all_least, all_most) = found.get_or_insert_with(|| (Vec::new(), Vec::new()));
            for (column, (min, max)) in least.into_iter().zip(most).enumerate() {
                if column == all_least.len() {
                    all_least.push(0.0);
                    all_most.push(0.0);
                }
                all_least[column] = all_least[column].max(min);
                all_most[column] = all_most[column].max(max);
            }
        }
        found
    }

    /// The text of a cell at (`row`, `column`) of `grid`, laid out in
    /// `width` pixels.
    fn cell_text(
        &self,
        grid: &Grid,
        (row, column): (usize, usize),
        content: &[Inline],
        width: f32,
        wrap: text::Wrap,
        ink: Ink,
    ) -> Paragraph {
        let bold = (grid.header_row && row == 0) || (grid.header_column && column == 0);
        let style = text::Look {
            size: self.look.size,
            weight: if bold { self.look.spacing.bold } else { 400.0 },
            color: self.look.text,
            wrap,
            align: match grid.alignments.get(column) {
                Some(Alignment::Center) => text::Align::Centre,
                Some(Alignment::Right) => text::Align::Right,
                _ => text::Align::Left,
            },
            ..text::Look::PLAIN
        };
        text::lay_out(self.fonts, &runs(content, self.look, ink), &style, width)
    }

    /// Lays `grid` out at `x` and the current top, its columns `widths`
    /// wide, and moves below it. On a shaded row of another table (the flow
    /// is `shaded`) it paints its unshaded rows in the page's colour too.
    #[expect(clippy::too_many_lines)]
    fn grid(&mut self, grid: &Grid, x: f32, widths: &[f32], wrap: text::Wrap) {
        let look = *self.look;
        let (across, down) = (
            look.spacing.cell_across * look.size,
            look.spacing.cell_down * look.size,
        );
        let columns = widths.len();
        let extra = 2.0 * across + 1.0;
        let table_width = widths.iter().sum::<f32>() + columns as f32 * extra + 1.0;
        let top = self.y;
        let mut y = top;
        let mut rows = Vec::new();
        for (row, cells) in grid.rows.iter().enumerate() {
            // Each cell is laid out where it goes, apart from the rest of
            // the row, whose shading goes under all of them.
            let mut inners = Vec::new();
            let mut left = x;
            let mut height = 0.0f32;
            // Every second body row, from the second: a header is none.
            let body = row.checked_sub(usize::from(grid.header_row));
            let shaded = body.is_some_and(|body| body % 2 == 1);
            for (column, &width) in widths.iter().enumerate() {
                let mut inner = self.child(self.look, y + down);
                // What a table in this cell is on, shaded or not.
                inner.shaded = shaded;
                if let Some(cell) = cells.get(column) {
                    let cell_x = left + 1.0 + across;
                    height = height.max(inner.cell(grid, (row, column), cell, cell_x, width, wrap));
                }
                inners.push(inner);
                left += width + extra;
            }
            let height = height + 2.0 * down;
            let paint = match (look.stripe, shaded) {
                (Some(stripe), true) => Some(stripe),
                (Some(_), false) if self.shaded => Some(look.page),
                _ => None,
            };
            if let Some(color) = paint {
                // In a frame the first row's top corners and the last row's
                // bottom corners are round, as the frame is.
                let round = look.spacing.code_radius * look.size;
                let (first, last) = (row == 0, row + 1 == grid.rows.len());
                let corner = |at: bool| {
                    if at && look.table_style == TableStyle::Frame {
                        round
                    } else {
                        0.0
                    }
                };
                let radii = [corner(first), corner(first), corner(last), corner(last)];
                match radii == [0.0; 4] {
                    true => self.rect(x, y, table_width, height, color),
                    false => self.push(
                        y..y + height,
                        Shape::Stripe {
                            x,
                            y,
                            width: table_width,
                            height,
                            radii,
                            color,
                        },
                    ),
                }
            }
            for inner in &mut inners {
                self.take(inner);
            }
            rows.push(y);
            y += height;
        }
        rows.push(y);
        // The borders, over the shading.
        match look.table_style {
            TableStyle::Grid => {
                for &line in &rows {
                    self.rect(x, line.round(), table_width, 1.0, look.table_border);
                }
                let mut left = x;
                self.rect(left.round(), top, 1.0, y - top + 1.0, look.table_border);
                for width in widths {
                    left += width + extra;
                    self.rect(left.round(), top, 1.0, y - top + 1.0, look.table_border);
                }
            }
            TableStyle::Frame => {
                // A rounded frame, a line under the header row and lines
                // between the columns.
                let (frame_top, bottom) = (top.round(), y.round() + 1.0);
                self.push(
                    frame_top..bottom,
                    Shape::Frame {
                        x: x.round(),
                        y: frame_top,
                        width: (table_width).round(),
                        height: bottom - frame_top,
                        radius: look.spacing.code_radius * look.size,
                        color: look.table_border,
                    },
                );
                if grid.header_row && rows.len() > 2 {
                    let line = rows[1].round();
                    self.rect(
                        x.round() + 1.0,
                        line,
                        table_width.round() - 2.0,
                        1.0,
                        look.table_border,
                    );
                }
                let mut left = x;
                for width in &widths[..widths.len().saturating_sub(1)] {
                    left += width + extra;
                    self.rect(
                        left.round(),
                        frame_top + 1.0,
                        1.0,
                        bottom - frame_top - 2.0,
                        look.table_border,
                    );
                }
            }
        }
        self.y = y + 1.0;
    }

    /// Lays `cell`, at (`row`, `column`) of `grid`, out at `x` and the
    /// current top in `width` pixels, and returns its height.
    fn cell(
        &mut self,
        grid: &Grid,
        (row, column): (usize, usize),
        cell: &Cell,
        x: f32,
        width: f32,
        wrap: text::Wrap,
    ) -> f32 {
        let top = self.y;
        match cell {
            Cell::Leaf(content) => {
                let ink = self.ink(width);
                let paragraph = self.cell_text(grid, (row, column), content, width, wrap, ink);
                self.y += self.text(paragraph, x);
            }
            Cell::Table(inner) => self.nested(inner, x, width, false),
            Cell::Stack(cells) => {
                // The tables of a stack have the same columns.
                let shared = self
                    .stack_columns(cells, self.ink(width))
                    .map(|(least, most)| {
                        let extra = self.look.size * 2.0 * self.look.spacing.cell_across + 1.0;
                        let room = width - least.len() as f32 * extra - 1.0;
                        let (widths, forced) = share(&least, &most, room);
                        let wrap = match forced {
                            true => text::Wrap::Forced,
                            false => text::Wrap::Words,
                        };
                        (widths, wrap)
                    });
                for (number, cell) in cells.iter().enumerate() {
                    // A gap between tables, as between blocks.
                    let table = |cell: &Cell| matches!(cell, Cell::Table(_));
                    if number > 0 && (table(cell) || table(&cells[number - 1])) {
                        self.y += self.look.size;
                    }
                    match (cell, &shared) {
                        (Cell::Table(inner), Some((widths, wrap))) => {
                            let columns = inner.columns();
                            self.grid(inner, x, &widths[..columns], *wrap);
                        }
                        _ => {
                            self.cell(grid, (row, column), cell, x, width, wrap);
                        }
                    }
                }
            }
        }
        self.y - top
    }

    /// Lays a table that is in a cell out at `x`, in `width` pixels: it
    /// wraps and never scrolls.
    /// With `fill`, as wide as `width`: the spare room goes to the columns
    /// in proportion to their content, as a browser's table at `width:
    /// 100%` shares it.
    pub(super) fn nested(&mut self, grid: &Grid, x: f32, width: f32, fill: bool) {
        let columns = grid.columns();
        if columns == 0 {
            return;
        }
        let ink = self.ink(width);
        let (least, most) = self.grid_widths(grid, ink);
        let extra = self.look.size * 2.0 * self.look.spacing.cell_across + 1.0;
        let room = width - columns as f32 * extra - 1.0;
        let (mut widths, forced) = share(&least, &most, room);
        let spare = room - widths.iter().sum::<f32>();
        let content: f32 = most.iter().sum();
        if fill && !forced && spare > 0.0 && content > 0.0 {
            for (width, max) in widths.iter_mut().zip(&most) {
                *width += spare * max / content;
            }
        }
        let wrap = if forced {
            text::Wrap::Forced
        } else {
            text::Wrap::Words
        };
        self.grid(grid, x, &widths, wrap);
    }
}

/// The widths of columns whose content is at least `least` and at most
/// `most` wide, sharing `room` as a browser's automatic table layout does:
/// as wide as their content when it fits; otherwise each at least its
/// least width, and the room left shared in proportion to how much each
/// could still grow. When even the least widths do not fit, each column
/// gets its share of the room in proportion to its least width, and the
/// second value says that words must then break inside to fit. Least widths
/// that are over the room by less than [`FITS`] still fit.
///
/// This is the pixel twin of `shrink` in `layout/table.rs`, which shares the
/// same room over whole cells, rounding by largest remainder instead of in
/// f32. Where the floors do not fit, `shrink` returns false and leaves the
/// widths alone, where this returns proportional widths and breaks words.
pub fn share(least: &[f32], most: &[f32], room: f32) -> (Vec<f32>, bool) {
    let (sum_least, sum_most) = (least.iter().sum::<f32>(), most.iter().sum::<f32>());
    if sum_most <= room {
        return (most.to_vec(), false);
    }
    // The words may add up to a hair over the room, by less than what is
    // rounding; they fit, and no word breaks.
    if sum_least <= room + FITS {
        let flex: f32 = least.iter().zip(most).map(|(min, max)| max - min).sum();
        let spare = (room - sum_least).max(0.0);
        let widths = (least.iter().zip(most))
            .map(|(min, max)| match flex > 0.0 {
                true => min + spare * (max - min) / flex,
                false => *min,
            })
            .collect();
        return (widths, false);
    }
    let room = room.max(0.0);
    let widths = (least.iter())
        .map(|min| match sum_least > 0.0 {
            true => min * room / sum_least,
            false => room / least.len() as f32,
        })
        .collect();
    (widths, true)
}

#[cfg(test)]
mod tests {
    use super::share;

    #[test]
    fn columns_are_as_wide_as_their_content_when_it_fits() {
        assert_eq!(
            share(&[10.0, 20.0], &[30.0, 40.0], 100.0),
            (vec![30.0, 40.0], false)
        );
    }

    #[test]
    fn the_spare_room_goes_to_the_columns_that_can_grow() {
        // 30 to share: the first column could grow by 20, the second by 40.
        let (widths, forced) = share(&[10.0, 20.0], &[30.0, 60.0], 60.0);
        assert_eq!(widths, [20.0, 40.0]);
        assert!(!forced);
    }

    #[test]
    fn columns_too_wide_even_wrapped_share_the_room_and_break_words() {
        let (widths, forced) = share(&[100.0, 300.0], &[200.0, 600.0], 200.0);
        assert_eq!(widths, [50.0, 150.0]);
        assert!(forced);
    }
}
