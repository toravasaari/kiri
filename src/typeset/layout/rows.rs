//! Long tables in the cell layout of the pager: a table of more rows than a
//! stretch is laid out a stretch of rows at a time, as a large code block
//! is, and the stretches far from the view are only measured. The widths of
//! its columns, or the choice to stack it, are made at its first stretch,
//! from what the columns of that stretch ask for and what was measured over
//! all its rows and is carried in `Inputs`; a wider cell later wraps.

use std::rc::Rc;

use crate::typeset::block::BlockId;
use crate::typeset::driver::Stack;
use crate::typeset::grid::Cell;
use crate::typeset::layout::table::{CellText, Content, Heights, Part, Wants, drawn_part, fit};
use crate::typeset::layout::{Cells, stamp, with_margin};
use crate::typeset::model::Inline;
use crate::typeset::stretch::Rows;
use crate::typeset::style::{Line, Measured, Span, Style};
use crate::typeset::text::Cursor;

/// The long table whose stretches the pen is laying out, between its first
/// stretch and its last: what one stretch leaves to the next, owned.
#[derive(Debug, Clone)]
pub(crate) struct Open {
    /// The table's one entry in the table of blocks.
    id: BlockId,
    /// The widths of its columns, chosen at its first stretch; `None` when
    /// it is stacked.
    widths: Option<Vec<usize>>,
    /// The cells of its header row when it is stacked, whose labels the
    /// measure of a row needs: made once.
    header: Vec<Cell<Content>>,
    /// The number among the texts of the table of the first cell of the
    /// next stretch.
    text: usize,
}

impl Cells<'_> {
    /// Lays the stretch `stretch` of a long table out and stacks it, joined
    /// to the stretch before it. The first stretch adds the table to the
    /// table of blocks and chooses the widths of its columns for all.
    pub(super) fn rows(&mut self, stretch: Rows, path: Vec<usize>, stack: &mut Stack) {
        let layout = self.layout;
        let (table, rows) = (stretch.table, stretch.rows());
        let grid = match stretch.first {
            true => {
                let file = layout.document;
                layout.marks.borrow_mut().path.clone_from(&path);
                let id = layout.add_block();
                let text = table.first_text(0);
                let grid = layout.table_cells(table, rows.clone(), text);
                let mut wants = Wants::of(&grid);
                // What was measured over all the rows, while the table has
                // as many columns.
                let measured = (layout
                    .tables
                    .and_then(|tables| tables.measured(file, &path)))
                .filter(|measured| measured.columns() == wants.columns());
                if let Some(measured) = measured {
                    wants.widen(measured);
                }
                let widths = fit(&wants, self.inner);
                let header = match widths {
                    Some(_) => Vec::new(),
                    None => grid.rows[0].clone(),
                };
                self.rowing = Some(Open {
                    id,
                    widths,
                    header,
                    text,
                });
                grid
            }
            false => {
                let text = self.rowing.as_ref().map_or(0, |open| open.text);
                layout.table_cells(table, rows.clone(), text)
            }
        };
        let Some(open) = &mut self.rowing else {
            unreachable!("a stretch after the first goes on from an open table");
        };
        open.text += grid.body().iter().map(Vec::len).sum::<usize>();
        let styles = layout.table_styles(layout.table_header());
        let part = Part {
            first: stretch.first,
            last: stretch.last,
            from: rows.start,
            body: table.row_count(),
        };
        let (lines, _) = drawn_part(&grid, open.widths.as_deref(), self.inner, &styles, part);
        let mut lines = with_margin(lines, self.lead);
        stamp(&mut lines, open.id);
        match stretch.first {
            true => stack.push(lines),
            false => stack.join(lines),
        }
        if stretch.last {
            self.rowing = None;
        }
    }

    /// Stacks as many lines as [`Cells::rows`] would for `stretch`, which is
    /// neither the first nor the last of its table, each only measured: the
    /// cells are wrapped at the widths of the table, or counted without that
    /// when they are plain text that fits, and no line is drawn. The cells
    /// of a CSV's table are read from its source as texts, and none is made.
    pub(super) fn measure_rows(&mut self, stretch: Rows, stack: &mut Stack) {
        let layout = self.layout;
        let table = stretch.table;
        let Some(open) = &mut self.rowing else {
            unreachable!("a stretch after the first goes on from an open table");
        };
        let styles = layout.table_styles(layout.table_header());
        // The cursor of the text is the pen's, which measuring leaves as it is.
        let cursor = layout.cursor.replace(Cursor::off());
        let columns = table.alignments.len().max(open.header.len());
        let heights = Heights::new(
            &open.header,
            columns,
            open.widths.as_deref(),
            self.inner,
            &styles,
        );
        let mut texts = vec![u32::try_from(open.text).unwrap_or(u32::MAX)];
        let mut lines = vec![0_u32];
        let mut counts: Vec<usize> = Vec::with_capacity(columns);
        let mut index = stretch.from;
        let mut total = 0;
        let mut text = open.text;
        let mut end_row = |counts: &mut Vec<usize>, cells: usize| {
            total += heights.before(index) + heights.row(counts.drain(..));
            text += cells;
            index += 1;
            lines.push(u32::try_from(total).unwrap_or(u32::MAX));
            texts.push(u32::try_from(text).unwrap_or(u32::MAX));
        };
        match table.source() {
            Some(source) => {
                let last = source.columns().saturating_sub(1);
                let rows = stretch.from + 1..stretch.end + 1;
                source.texts(rows, |_, column, cell| {
                    counts.push(heights.cell(column, CellText::Text(cell)));
                    if column == last {
                        end_row(&mut counts, last + 1);
                    }
                });
            }
            None => {
                for row in table.rows(stretch.rows()).iter() {
                    for (column, cell) in row.iter().enumerate() {
                        let cell = match plain(cell) {
                            Some(text) => CellText::Text(text),
                            None => {
                                let mut spans: Vec<Span> = Vec::new();
                                layout.inlines(cell, &Style::default(), &mut spans);
                                CellText::Spans(spans)
                            }
                        };
                        counts.push(heights.cell(column, cell));
                    }
                    end_row(&mut counts, row.len());
                }
            }
        }
        layout.cursor.set(cursor);
        open.text = text;
        let measured = Rc::new(Measured::Rows { texts, lines });
        stack.join(vec![Line::measured(open.id, measured); total]);
    }
}

/// The text of a cell that is only that, plain: no inline but a text, or
/// none.
fn plain(cell: &[Inline]) -> Option<&str> {
    match cell {
        [] => Some(""),
        [Inline::Text(text)] => Some(text),
        _ => None,
    }
}
