//! Long tables in the cell layout of the pager: a table of more rows than a
//! stretch is laid out a stretch of rows at a time, as a large code block
//! is, and the stretches far from the view are only measured. The widths of
//! its columns, or the choice to stack it, are made at its first stretch,
//! from what the columns ask for over all its rows: a CSV's as its parse
//! found them, which the pager carries in `Inputs` as `:w` would; a
//! Markdown table's of at most `FIT_CELLS` cells measured at once. A longer
//! Markdown table takes what its first stretch asks for until `:w`, and what
//! is carried stays through a reload of follow mode, so that a wider cell
//! that comes wraps.

use std::ops::Range;
use std::rc::Rc;

use crate::typeset::block::BlockId;
use crate::typeset::driver::Stack;
use crate::typeset::grid::Cell;
use crate::typeset::layout::Layout;
use crate::typeset::layout::table::{CellText, Content, Heights, Part, Wants, drawn_part, fit};
use crate::typeset::layout::{Cells, stamp, with_margin};
use crate::typeset::model::Inline;
use crate::typeset::model::Table;
use crate::typeset::source_rows::{SourceRows, cell};
use crate::typeset::stretch;
use crate::typeset::stretch::Rows;
use crate::typeset::style::{Line, Measured, Span, Style};
use crate::typeset::text::Cursor;

/// The fewest cells a column of a table laid out in stretches takes: a
/// letter two cells wide, as a CJK letter or an emoji, fits it.
const MIN_COLUMN: usize = 2;

/// The most cells a long Markdown table has that is fitted to all its rows
/// at its first layout, as it is printed; a longer one takes the widths of
/// its first stretch until `:w`. Its cells are laid out to measure them, at
/// some microseconds a cell.
const FIT_CELLS: usize = 20_000;

/// Whether the first layout of `table`, a long one, fits it to all its
/// rows: a Markdown table of at most `FIT_CELLS` cells. Then `:w` and `r`
/// have nothing to measure of it.
fn fitted_at_first(table: &Table) -> bool {
    table.source().is_none() && cells(table) <= FIT_CELLS
}

/// How many cells `table` has, its header's with them: for a table whose
/// rows are as many as its alignments, as a Markdown table's are.
fn cells(table: &Table) -> usize {
    table.alignments.len().saturating_mul(table.row_count() + 1)
}

#[cfg(test)]
thread_local! {
    /// How many times what a CSV's parse found was taken in on this thread.
    static SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times what a CSV's parse found of its columns was taken in on
/// this thread.
#[cfg(test)]
pub(crate) fn scans() -> usize {
    SCANS.with(std::cell::Cell::get)
}

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
                match fitted_at_first(table) {
                    // Fitted to all its rows at once, as it is printed: they
                    // are parsed, and cannot change without a new layout, so
                    // it needs no room for more.
                    true => wants = layout.all_wants(table),
                    // A column is two cells wide at least, so that a letter
                    // two cells wide in a later row fits it, as no row past
                    // the first stretch widens it.
                    false => {
                        if let Some(measured) = measured {
                            wants.widen(measured);
                        }
                        wants.at_least(MIN_COLUMN);
                    }
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

impl Cells<'_> {
    /// What the columns of `table` ask for over all its rows, its header's
    /// included, as the cells of a stretch of it ask: a CSV's read from its
    /// source as texts, without making a cell, and a Markdown table's laid
    /// out. A table of no more rows than a stretch asks for nothing, as it
    /// is laid out whole, and nor does one that its first layout fits to
    /// all its rows.
    pub(crate) fn wants(&self, table: &Table) -> Option<Wants> {
        if !stretch::is_long(table, stretch::LINES) || fitted_at_first(table) {
            return None;
        }
        Some(self.layout.wants(table))
    }
}

impl Layout<'_> {
    /// What the columns of `table` ask for over all its rows, each two
    /// cells at least, as what `:w` measures is used with rows that come
    /// later. A CSV's are those the parse found in its pass over the rows,
    /// with the rows of fields that are not one line of text measured as
    /// cells are.
    fn wants(&self, table: &Table) -> Wants {
        let mut wants = match table.source() {
            Some(source) => self.scanned_wants(source),
            None => self.all_wants(table),
        };
        wants.at_least(MIN_COLUMN);
        wants
    }

    /// What the columns of a CSV's table ask for, from what the parse found
    /// of their fields, and the rows it left to measure read again.
    fn scanned_wants(&self, source: &SourceRows) -> Wants {
        #[cfg(test)]
        SCANS.with(|scans| scans.set(scans.get() + 1));
        let texts = source.column_texts();
        let mut wants = Wants::of_texts(&texts.widest, &texts.word, source.columns());
        // The rows to measure are read in one pass, from the first to the
        // last of them, which costs one reader however many they are.
        if let (Some(&first), Some(&last)) = (texts.other.first(), texts.other.last()) {
            let mut other = texts.other.iter().copied().peekable();
            let columns = &texts.other_columns;
            let mut wanted = |row: usize, column: usize| {
                if column == 0 {
                    while other.next_if(|&next| next < row).is_some() {}
                }
                other.peek() == Some(&row) && columns.contains(&column)
            };
            self.read_wants(source, first..last + 1, &mut wanted, &mut wants);
        }
        wants
    }

    /// What the columns of `table` ask for over all its rows, each cell
    /// measured.
    fn all_wants(&self, table: &Table) -> Wants {
        let mut wants = Wants::new(table.alignments.len());
        if let Some(source) = table.source() {
            self.read_wants(source, 0..source.len(), &mut |_, _| true, &mut wants);
            return wants;
        }
        let (header_style, body_style) = (self.table_header(), Style::default());
        let cursor = self.cursor.replace(Cursor::off());
        let header = table.header();
        let rows = table.rows(0..table.row_count());
        let all = std::iter::once((&*header, &header_style))
            .chain(rows.iter().map(|row| (row.as_slice(), &body_style)));
        for (row, style) in all {
            for (column, inlines) in row.iter().enumerate() {
                match plain(inlines) {
                    Some(text) => wants.add_text(column, text, || self.spans(inlines, style)),
                    None => wants.add(column, &Cell::from(self.spans(inlines, style))),
                }
            }
        }
        self.cursor.set(cursor);
        wants
    }

    /// Takes the cells of the rows `rows` of a CSV's table that `wanted`
    /// says, by row and column, read from its source as texts, into
    /// `wants`.
    fn read_wants(
        &self,
        source: &SourceRows,
        rows: Range<usize>,
        wanted: &mut dyn FnMut(usize, usize) -> bool,
        wants: &mut Wants,
    ) {
        let (header_style, body_style) = (self.table_header(), Style::default());
        let cursor = self.cursor.replace(Cursor::off());
        source.texts(rows, |row, column, text| {
            if !wanted(row, column) {
                return;
            }
            let style = match row {
                0 => &header_style,
                _ => &body_style,
            };
            wants.add_text(column, text, || self.spans(&cell(text), style));
        });
        self.cursor.set(cursor);
    }

    /// The spans the layout makes of `inlines` in `style`.
    fn spans(&self, inlines: &[Inline], style: &Style) -> Vec<Span> {
        let mut spans = Vec::new();
        self.inlines(inlines, style, &mut spans);
        spans
    }
}

#[cfg(test)]
mod tests {
    use crate::render::Inputs;
    use crate::typeset::block::Blocks;
    use crate::typeset::caps::Capabilities;
    use crate::typeset::layout::table::Wants;
    use crate::typeset::layout::{Derived, with_cells};
    use crate::typeset::model::Block;
    use crate::typeset::parse::csv::{self, Separator};

    /// Fields of every kind: empty and space only, wide, combining and
    /// joined letters, no-break, zero-width and ideographic spaces, a soft
    /// hyphen, a line separator, controls, long words, and quoted fields
    /// with a tab, a line break, a comma and a quote.
    const FIELDS: &[&str] = &[
        "",
        " ",
        "   ",
        "a",
        "ab cd",
        "  lead",
        "trail  ",
        "a  b",
        "日本語",
        "日本語の 文字",
        "😀",
        "👨\u{200d}👩\u{200d}👧",
        "e\u{301}",
        "\u{a0}",
        "a\u{a0}b",
        "a\u{200b}b",
        "longwordwithoutspaces1234567890",
        "x y z w v",
        "\u{2028}",
        "a\u{2028}b",
        "\"q\tb\"",
        "\"l1\nl2\"",
        "\"l1\r\nl2\"",
        "\"a, b\"",
        "\"say \"\"hi\"\"\"",
        "Київ",
        "12345",
        "user1000@example.com",
        "🇫🇮",
        "\u{ad}",
        "a\u{3000}b",
        "\u{feff}x",
        "\u{202e}abc",
        "❤\u{fe0f}",
        "ｱ",
        "\u{7f}",
        "a\u{1b}[31mb",
    ];

    /// A CSV of `rows` rows of `columns` columns of `FIELDS`, some short and
    /// one long, with a byte order mark at the start when `bom`.
    fn source(rows: usize, columns: usize, separator: char, bom: bool) -> String {
        let mut source = match bom {
            true => "\u{feff}".to_owned(),
            false => String::new(),
        };
        let mut next = 7usize;
        for row in 0..rows {
            let fields = match row % 11 {
                3 => 1,
                7 => columns + 2,
                _ => columns,
            };
            let row: Vec<String> = (0..fields)
                .map(|_| {
                    next = (next * 31 + 17) % 1009;
                    let field = FIELDS[next % FIELDS.len()];
                    match separator {
                        '\t' => field.replace('\t', " "),
                        _ => field.to_owned(),
                    }
                })
                .collect();
            source += &row.join(&separator.to_string());
            source.push('\n');
        }
        source
    }

    #[test]
    fn what_the_parse_finds_of_the_columns_is_what_measuring_the_cells_finds() {
        let inputs = Inputs::new(Capabilities::kitty(80));
        let derived = Derived::new(&inputs);
        for (columns, separator, bom) in [(1, ',', false), (4, ',', true), (7, '\t', false)] {
            let separators = match separator {
                '\t' => Separator::Tab,
                _ => Separator::Comma,
            };
            let source = source(300, columns, separator, bom);
            let document = csv::parse(&source, separators).unwrap();
            let Block::Table(table) = &document.blocks[0] else {
                panic!("a table")
            };
            let source_rows = table.source().unwrap();
            assert!(!source_rows.column_texts().other.is_empty());
            let mut blocks = Blocks::default();
            let (scanned, all) = with_cells(&inputs, &derived, (0, 0), &mut blocks, |cells| {
                let layout = cells.layout;
                (layout.scanned_wants(source_rows), layout.all_wants(table))
            });
            assert_eq!(scanned, all, "{columns} columns, {separator:?}");
        }
    }

    /// A CSV of `rows` rows whose note has `field` in it in every row.
    fn with_notes(rows: usize, field: &str) -> String {
        let mut source = "id,name,note\n".to_owned();
        for row in 0..rows {
            source += &format!(
                "{row},name {row},\"{}\"\n",
                field.replace('#', &row.to_string())
            );
        }
        source
    }

    /// What the parse found, and what measuring every cell finds, of the
    /// table of `source`.
    fn found_and_measured(source: &str) -> (Vec<usize>, Wants, Wants) {
        let inputs = Inputs::new(Capabilities::kitty(80));
        let derived = Derived::new(&inputs);
        let document = csv::parse(source, Separator::Comma).unwrap();
        let Block::Table(table) = &document.blocks[0] else {
            panic!("a table")
        };
        let source_rows = table.source().unwrap();
        let mut blocks = Blocks::default();
        let (scanned, all) = with_cells(&inputs, &derived, (0, 0), &mut blocks, |cells| {
            let layout = cells.layout;
            (layout.scanned_wants(source_rows), layout.all_wants(table))
        });
        (source_rows.column_texts().other.clone(), scanned, all)
    }

    #[test]
    fn a_line_break_in_a_field_is_the_space_it_is_in_its_cell() {
        for note in ["line one of #\nline two", "a\r\nb #", "c\rd #", "e\n\nf #"] {
            let (other, scanned, all) = found_and_measured(&with_notes(3000, note));
            assert_eq!(other, Vec::<usize>::new(), "{note:?}");
            assert_eq!(scanned, all, "{note:?}");
        }
    }

    #[test]
    fn the_rows_with_a_tab_are_measured_again_in_one_pass() {
        use crate::typeset::source_rows::readers_made;
        for (note, every) in [("tab\there #", 1), ("tab\there #", 3), ("x\u{7}y #", 2)] {
            let mut source = "id,name,note\n".to_owned();
            for row in 0..3000 {
                let note = match row % every {
                    0 => note.replace('#', &row.to_string()),
                    _ => format!("plain {row}"),
                };
                source += &format!("{row},name {row},\"{note}\"\n");
            }
            let before = readers_made();
            let (other, scanned, all) = found_and_measured(&source);
            // The parse's reader, the reader of the rows to measure again,
            // and those of measuring every cell for the test.
            let made = readers_made() - before;
            assert!(other.len() >= 1000, "{note:?}: {}", other.len());
            assert!(made <= 3, "{note:?} every {every}: {made} readers");
            assert_eq!(scanned, all, "{note:?}");
        }
    }
}
