//! Tables drawn with box-drawing characters, or stacked when too narrow.

use std::ops::Range;

use unicode_width::UnicodeWidthStr;

use crate::typeset::color::{Color, Underline};
use crate::typeset::grid::{Cell, Grid};
use crate::typeset::model::Alignment;
use crate::typeset::size::TextSize;
use crate::typeset::style::{self, Line, Spaces, Span, Style, push};
use crate::typeset::wrap::{keeps_together, plain_rows, split, width, words, wrap};

/// The leaf of a text layout cell: what a cell holds when it is not a
/// table or a stack.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// Text, wrapped to the column.
    Text(Vec<Span>),
    /// Lines of code, kept as written and split where they are too long,
    /// each filled to the column's width in `fill`, as a code block is.
    Code { lines: Vec<Vec<Span>>, fill: Style },
}

impl From<Vec<Span>> for Cell<Content> {
    fn from(spans: Vec<Span>) -> Self {
        Self::Leaf(Content::Text(spans))
    }
}

impl Cell<Content> {
    /// Cells it takes without wrapping.
    fn natural(&self) -> usize {
        match self {
            Self::Leaf(Content::Text(spans)) => widest_line(spans),
            Self::Table(cells) => natural_width(cells),
            Self::Stack(parts) => parts.iter().map(Cell::natural).max().unwrap_or(0),
            Self::Leaf(Content::Code { lines, .. }) => code_width(lines),
        }
    }

    /// The fewest cells it takes without splitting a short word. A table
    /// that narrow is stacked.
    fn floor(&self) -> usize {
        match self {
            Self::Leaf(Content::Text(spans)) => longest_word(spans).min(SHORT_WORD),
            Self::Table(cells) => Wants::of(cells).floor.into_iter().max().unwrap_or(0),
            Self::Stack(parts) => parts.iter().map(Cell::floor).max().unwrap_or(0),
            Self::Leaf(Content::Code { lines, .. }) => code_width(lines).min(SHORT_WORD),
        }
    }

    /// The cells it would rather keep on one line: text kept together, or
    /// a table as a box.
    fn together(&self) -> usize {
        match self {
            Self::Leaf(Content::Text(spans)) => longest_together(spans),
            Self::Table(cells) => boxed_floor(cells),
            Self::Stack(parts) => parts.iter().map(Cell::together).max().unwrap_or(0),
            // Code split at the edge reads badly.
            Self::Leaf(Content::Code { lines, .. }) => code_width(lines),
        }
    }

    /// Its lines in `max` cells, and whether the last of them is the bottom
    /// border of a table drawn as a box.
    fn drawn(&self, max: usize, styles: &Styles) -> (Vec<Vec<Span>>, bool) {
        match self {
            Self::Leaf(Content::Text(spans)) if styles.rows == Rows::Indent => {
                (hanging(spans, max), false)
            }
            Self::Leaf(Content::Text(spans)) => (wrap(spans, max), false),
            Self::Table(cells) => {
                let (lines, boxed) = drawn_table(cells, max, styles);
                (lines.into_iter().map(|line| line.spans).collect(), boxed)
            }
            Self::Stack(parts) => stack_lines(parts, max, styles),
            Self::Leaf(Content::Code { lines, fill }) => {
                let lines = (lines.iter())
                    .flat_map(|line| split(line, max))
                    .map(|mut line| {
                        let used = width(&line);
                        push(&mut line, &" ".repeat(max.saturating_sub(used)), fill);
                        line
                    })
                    .collect();
                (lines, false)
            }
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Leaf(Content::Text(spans)) => is_empty(spans),
            Self::Table(grid) => grid.body().is_empty(),
            Self::Stack(parts) => parts.iter().all(Cell::is_empty),
            Self::Leaf(Content::Code { lines, .. }) => lines.iter().all(|line| is_empty(line)),
        }
    }
}

/// The width of the longest line of code.
fn code_width(lines: &[Vec<Span>]) -> usize {
    lines.iter().map(|line| width(line)).max().unwrap_or(0)
}

/// The lines of a stack in `max` cells. Its tables are all boxes, or all
/// stacked when one of them does not fit as a box, so that they look
/// alike; stacked, its parts are separated by rules. With them, whether
/// the last line is the bottom border of a box.
fn stack_lines(parts: &[Cell<Content>], max: usize, styles: &Styles) -> (Vec<Vec<Span>>, bool) {
    let boxed = stack_is_boxed(parts, max);
    let mut lines = Vec::new();
    let mut ends_in_box = false;
    for (index, part) in parts.iter().enumerate() {
        if !boxed && index > 0 {
            lines.push(vec![Span::new(&"─".repeat(max), styles.border.clone())]);
            ends_in_box = false;
        }
        let (part_lines, part_ends) = match part {
            Cell::Table(cells) if !boxed => {
                let part = Part::whole(cells.body().len());
                let (stacked, ends) = stacked(cells, cells.columns(), max, styles, part);
                (stacked.into_iter().map(|line| line.spans).collect(), ends)
            }
            part => part.drawn(max, styles),
        };
        if !part_lines.is_empty() {
            ends_in_box = part_ends;
        }
        lines.extend(part_lines);
    }
    (lines, ends_in_box)
}

/// Whether the tables of a stack in `max` cells are boxes.
fn stack_is_boxed(parts: &[Cell<Content>], max: usize) -> bool {
    (parts.iter()).all(|part| match part {
        Cell::Table(cells) => boxed_floor(cells) <= max,
        _ => true,
    })
}

#[derive(Clone)]
pub struct Styles {
    pub border: Style,
    /// For the colon after a header in a stacked table.
    pub header: Style,
    /// How the rows of a table drawn as a box are told apart.
    pub rows: Rows,
    /// Painted behind the table where nothing else is: the page colour
    /// for a table in a striped row, so that its own stripes show.
    pub base: Option<Color>,
}

/// How the rows of a table drawn as a box are told apart; a stacked table
/// keeps its own layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rows {
    /// Not at all.
    #[default]
    Plain,
    /// A rule between the body rows.
    Lines,
    /// The lines after the first of a text cell start two cells in.
    Indent,
    /// The last line of each body row but the last is underlined, dashed,
    /// in `color` or else the text colour, between the table's borders,
    /// except in a column where a nested table's box closes the row.
    Underline { color: Option<Color> },
    /// Every second body row on `stripe`, from border to border. A table
    /// in a striped row is drawn on `page`, so its stripes start afresh.
    Stripes { stripe: Color, page: Color },
}

/// A word up to this many cells is split only when the table cannot fit
/// otherwise; longer ones, such as URLs, may be split to make room.
const SHORT_WORD: usize = 10;

/// Text kept together, such as inline code with `inline_code_wrap` off,
/// stays whole only while every other column keeps this many cells, or
/// its own width when that is less.
const COMFORT: usize = 16;

/// Columns shrink in proportion to their text until the table fits in
/// `max` cells; cell text then wraps inside its column. A table that would
/// have to split short words is stacked instead.
pub fn layout(cells: &Grid<Content>, max: usize, styles: &Styles) -> Vec<Line> {
    drawn_table(cells, max, styles).0
}

/// `layout`'s lines, and whether the last of them is the bottom border of
/// a box: the table's own, or one in its last value when it is stacked.
fn drawn_table(cells: &Grid<Content>, max: usize, styles: &Styles) -> (Vec<Line>, bool) {
    debug_assert_ne!(cells.header_row, cells.header_column, "{FLAGS}");
    if cells.columns() == 0 {
        return (Vec::new(), false);
    }
    let widths = fit(&Wants::of(cells), max);
    drawn_part(
        cells,
        widths.as_deref(),
        max,
        styles,
        Part::whole(cells.body().len()),
    )
}

/// Which body rows of a table a grid holds, when it holds some of them only,
/// as a stretch of a long table does: the grid has the table's header row,
/// if it has one, and the rows from the body row `from` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part {
    /// Whether the part starts the table: its top border and its header row
    /// are drawn with it.
    pub first: bool,
    /// Whether the part ends the table: its bottom border is drawn after it.
    pub last: bool,
    /// The number of its first body row among the table's.
    pub from: usize,
    /// How many body rows the table has.
    pub body: usize,
}

impl Part {
    /// All the `body` rows of a table.
    pub fn whole(body: usize) -> Self {
        Self {
            first: true,
            last: true,
            from: 0,
            body,
        }
    }
}

/// The lines of the rows of `cells` that `part` says it holds, drawn as a box
/// with the column `widths`, or stacked when there are none, in `max` cells;
/// and whether the last line is the bottom border of a box. A table drawn
/// in parts that follow each other, with the same widths, is the table drawn
/// whole: the stripes, rules and underlines go by the number of a row among
/// all the table's.
pub fn drawn_part(
    cells: &Grid<Content>,
    widths: Option<&[usize]>,
    max: usize,
    styles: &Styles,
    part: Part,
) -> (Vec<Line>, bool) {
    let columns = cells.columns();
    if columns == 0 {
        return (Vec::new(), false);
    }
    let Some(widths) = widths else {
        return stacked(cells, columns, max, styles, part);
    };
    let border = &styles.border;
    let base = styles.base;
    let rule = |ends| painted(border_line(widths, ends, border), base);
    // The top and bottom borders are not painted: a rule is drawn through
    // the middle of its cells, so `base` behind them would show half a
    // cell outside the box.
    let edge = |ends| border_line(widths, ends, border);
    let mut lines = Vec::new();
    if part.first {
        lines.push(edge(["╭", "┬", "╮"]));
        if let Some(header) = cells.rows.first().filter(|_| cells.header_row) {
            lines.extend(row_lines(header, widths, &cells.alignments, styles, base));
            lines.push(rule(["├", "┼", "┤"]));
        }
    }
    for (at, row) in cells.body().iter().enumerate() {
        let index = part.from + at;
        let background = match styles.rows {
            Rows::Lines if index > 0 => {
                lines.push(rule(["├", "┼", "┤"]));
                base
            }
            Rows::Stripes { stripe, page } if index % 2 == 1 => {
                // Tables in the row start their stripes on the page.
                let styles = Styles {
                    base: Some(page),
                    ..styles.clone()
                };
                lines.extend(row_lines(
                    row,
                    widths,
                    &cells.alignments,
                    &styles,
                    Some(stripe),
                ));
                continue;
            }
            Rows::Underline { color } if index + 1 < part.body => {
                let (wrapped, ends) = wrap_row(row, widths, styles);
                let mut row_lines = join_row(&wrapped, widths, &cells.alignments, styles, base);
                if let Some(last) = row_lines.last_mut() {
                    underline(last, color, &boxed_spans(&wrapped, &ends));
                }
                lines.extend(row_lines);
                continue;
            }
            _ => base,
        };
        lines.extend(row_lines(
            row,
            widths,
            &cells.alignments,
            styles,
            background,
        ));
    }
    if part.last {
        lines.push(edge(["╰", "┴", "╯"]));
    }
    (lines, part.last)
}

/// The text layout has a header row or a header column, never both or
/// neither: a table has the one, front matter the other.
const FLAGS: &str = "a header row or a header column";

/// What each column of a table asks for, found over its cells: its widest
/// cell, its fewest cells and what it keeps together. The widths of a table
/// are chosen from these, so a table whose rows are not all at hand, as a
/// long table laid out in stretches, can carry them from a pass over all its
/// rows and choose its widths again for another width.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wants {
    /// The cells each column takes without wrapping.
    natural: Vec<usize>,
    /// The fewest cells each column takes without splitting a short word,
    /// or any word of a header column; at least one.
    floor: Vec<usize>,
    /// The cells each column would rather keep on one line.
    together: Vec<usize>,
}

impl Wants {
    /// What the columns of `cells` ask for.
    pub fn of(cells: &Grid<Content>) -> Self {
        let mut wants = Self::new(cells.columns());
        for row in &cells.rows {
            for (column, cell) in row.iter().enumerate() {
                wants.add(column, cell);
            }
        }
        if cells.header_column {
            for row in &cells.rows {
                if let Some(Cell::Leaf(Content::Text(header))) = row.first() {
                    wants.floor[0] = wants.floor[0].max(longest_word(header));
                }
            }
        }
        wants
    }

    /// What `columns` columns ask for before any cell: nothing but a cell
    /// each.
    pub fn new(columns: usize) -> Self {
        Self {
            natural: vec![0; columns],
            floor: vec![1; columns],
            together: vec![0; columns],
        }
    }

    /// How many columns there are.
    pub fn columns(&self) -> usize {
        self.natural.len()
    }

    /// Takes in `cell`, of the column `column`.
    pub fn add(&mut self, column: usize, cell: &Cell<Content>) {
        self.take(column, cell.natural(), cell.floor(), cell.together());
    }

    /// Takes in a cell of the column `column` that is `text` alone, plain,
    /// in the style that `spans` would give it; `spans` makes the cell when
    /// the text cannot be measured as it is.
    pub fn add_text(&mut self, column: usize, text: &str, spans: impl FnOnce() -> Vec<Span>) {
        match plain_width(text) {
            Some(natural) => {
                let word = (text.split(' ').map(UnicodeWidthStr::width).max()).unwrap_or(0);
                self.take(column, natural, word.min(SHORT_WORD), 0);
            }
            None => self.add(column, &Cell::from(spans())),
        }
    }

    /// Takes in what a cell of `column` asks for.
    fn take(&mut self, column: usize, natural: usize, floor: usize, together: usize) {
        self.natural[column] = self.natural[column].max(natural);
        self.floor[column] = self.floor[column].max(floor);
        self.together[column] = self.together[column].max(together);
    }

    /// What `columns` columns ask for whose fields are one line of text
    /// each, as a CSV's parse found them: the cells of the widest of each
    /// column and of its longest run without a space.
    pub fn of_texts(widest: &[usize], word: &[usize], columns: usize) -> Self {
        let mut wants = Self::new(columns);
        for column in 0..columns.min(widest.len()) {
            wants.take(column, widest[column], word[column].min(SHORT_WORD), 0);
        }
        wants
    }

    /// Has every column ask for `cells` cells at least, to take and to keep.
    pub fn at_least(&mut self, cells: usize) {
        for column in 0..self.columns() {
            self.take(column, cells, cells, 0);
        }
    }

    /// Takes in what the columns of `other`, as many, ask for.
    pub fn widen(&mut self, other: &Self) {
        debug_assert_eq!(self.columns(), other.columns());
        for column in 0..self.columns().min(other.columns()) {
            let (natural, floor) = (other.natural[column], other.floor[column]);
            self.take(column, natural, floor, other.together[column]);
        }
    }
}

/// The cells `text` takes as the text of a span when that is known without
/// making one: when it has no character that a span would show otherwise,
/// such as a tab or a line break, so that it is one line of `text.width()`
/// cells.
pub fn plain_width(text: &str) -> Option<usize> {
    match text.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
        true => Some(text.len()),
        false => (!text.chars().any(char::is_control)).then(|| text.width()),
    }
}

/// The width of the table drawn as a box without wrapping.
fn natural_width(cells: &Grid<Content>) -> usize {
    let wants = Wants::of(cells);
    wants
        .natural
        .iter()
        .map(|width| width.max(&1))
        .sum::<usize>()
        + 3 * wants.columns()
        + 1
}

/// The fewest cells the table takes as a box.
fn boxed_floor(cells: &Grid<Content>) -> usize {
    let wants = Wants::of(cells);
    wants.floor.iter().sum::<usize>() + 3 * wants.columns() + 1
}

/// The widths of the columns that `wants` asks for, which fit in `max`
/// without splitting short words, or `None`: then the table is stacked.
pub fn fit(wants: &Wants, max: usize) -> Option<Vec<usize>> {
    let columns = wants.columns();
    // No column is narrower than its longest short word. That covers its
    // widest character too, two cells for CJK and emoji, which would push
    // its borders out of line.
    let mut widths: Vec<usize> = wants.natural.iter().map(|&w| w.max(1)).collect();
    let minimums = &wants.floor;
    let together = &wants.together;
    // Each column costs its padding and one border, plus the closing border.
    let available = max.checked_sub(3 * columns + 1)?;
    if minimums.iter().sum::<usize>() > available {
        return None;
    }
    // Without room for that, the table is laid out as if the text wrapped
    // at its spaces.
    let comfortable: Vec<usize> = (0..columns)
        .map(|column| together[column].max(widths[column].min(COMFORT)))
        .collect();
    let mut kept = widths.clone();
    if shrink(&mut kept, &comfortable, available) {
        return Some(kept);
    }
    let fits = shrink(&mut widths, minimums, available);
    debug_assert!(fits, "the minimums fit");
    Some(widths)
}

/// Narrows the widths to fit in `available`, as a browser's automatic
/// table layout does: every column keeps its floor, and the room above
/// the floors is shared in proportion to how much each column wants above
/// its own. The cells left over by rounding down go to the largest
/// remainders, the leftmost column first on a tie. False if the floors do
/// not fit, and then `widths` is left as it was.
///
/// This is the whole-cell twin of `share` in `reading/page/table.rs`, which
/// shares the same room in exact f32 and lets a total over the room by less
/// than `FITS`, half a pixel, still fit. Where the floors do not fit, `share`
/// returns proportional widths and says that words must break instead.
pub(crate) fn shrink(widths: &mut [usize], floors: &[usize], available: usize) -> bool {
    if widths.iter().sum::<usize>() <= available {
        return true;
    }
    let Some(room) = available.checked_sub(floors.iter().sum()) else {
        return false;
    };
    let wants: Vec<usize> = (widths.iter().zip(floors))
        .map(|(width, floor)| width - floor)
        .collect();
    let wanted: usize = wants.iter().sum();
    let mut remainders = Vec::with_capacity(widths.len());
    for (column, want) in wants.iter().enumerate() {
        widths[column] = floors[column] + want * room / wanted;
        remainders.push((want * room % wanted, column));
    }
    let left = available - widths.iter().sum::<usize>();
    remainders.sort_by_key(|&(remainder, column)| (std::cmp::Reverse(remainder), column));
    for &(_, column) in &remainders[..left] {
        widths[column] += 1;
    }
    true
}

/// Each body row as a block of `Header: value` lines, separated by rules.
/// When the longest header leaves too little room for its value, every
/// value goes on the lines under its header. A table with a header column
/// takes the header of each row from it, without rules between the rows.
/// The rows are those that `part` says `cells` holds.
fn stacked(
    cells: &Grid<Content>,
    columns: usize,
    max: usize,
    styles: &Styles,
    part: Part,
) -> (Vec<Line>, bool) {
    debug_assert_ne!(cells.header_row, cells.header_column, "{FLAGS}");
    let empty = Cell::from(Vec::new());
    // A table with a header only still shows its headers.
    let no_row = [Vec::new()];
    let rows = match cells.body() {
        [] if cells.header_row => &no_row[..],
        body => body,
    };
    // Each row as its headers and its values.
    let entries: Vec<Vec<(Vec<Span>, &Cell<Content>)>> = if cells.header_column {
        (rows.iter())
            .map(|row| {
                let header = label(row.first(), styles);
                let values = row.iter().skip(1).chain(std::iter::repeat(&empty));
                let headers = std::iter::once(header).chain(std::iter::repeat(Vec::new()));
                headers.zip(values).take(columns.max(2) - 1).collect()
            })
            .collect()
    } else {
        let labels = labels(
            cells.rows.first().filter(|_| cells.header_row),
            columns,
            styles,
        );
        (rows.iter())
            .map(|row| {
                let values = row.iter().chain(std::iter::repeat(&empty));
                labels.iter().cloned().zip(values).collect()
            })
            .collect()
    };
    let longest = (entries.iter().flatten())
        .map(|(label, _)| width(label))
        .max()
        .unwrap_or(0);
    let tight = max < longest + 1 + SHORT_WORD;

    let mut lines = Vec::new();
    let mut ends_in_box = false;
    for (at, row) in entries.into_iter().enumerate() {
        if part.from + at > 0 && !cells.header_column {
            lines.push(Line::new(vec![Span::new(
                &"─".repeat(max),
                styles.border.clone(),
            )]));
            ends_in_box = false;
        }
        for (label, value) in row {
            let (entry_lines, ends) = entry(label, value, tight, max, styles);
            if let Some(ends) = ends {
                ends_in_box = ends;
            }
            lines.extend(entry_lines.into_iter().map(Line::new));
        }
    }
    (lines, ends_in_box)
}

/// The label of a stacked table's entry for the header `cell`: its text and
/// a colon, nothing for an empty header. A header cell that is not text,
/// which no table has, gets no label.
fn label(cell: Option<&Cell<Content>>, styles: &Styles) -> Vec<Span> {
    match cell {
        Some(Cell::Leaf(Content::Text(header))) => {
            let mut label = header.clone();
            if !is_empty(header) {
                push(&mut label, ":", &styles.header);
            }
            label
        }
        _ => Vec::new(),
    }
}

/// The labels of the `columns` columns of a stacked table whose header row
/// is `header`, if it has one.
fn labels(header: Option<&Vec<Cell<Content>>>, columns: usize, styles: &Styles) -> Vec<Vec<Span>> {
    (0..columns)
        .map(|column| label(header.and_then(|header| header.get(column)), styles))
        .collect()
}

/// The lines of one entry of a stacked table in `max` cells, its `label`
/// and its `value`, the value under the label when `tight`; and whether its
/// last line is the bottom border of a box, `None` when it has no line.
fn entry(
    label: Vec<Span>,
    value: &Cell<Content>,
    tight: bool,
    max: usize,
    styles: &Styles,
) -> (Vec<Vec<Span>>, Option<bool>) {
    let mut parts: Vec<Cell<Content>> = Vec::new();
    match (is_empty(&label), value.is_empty(), value) {
        (true, true, _) => {}
        (true, false, _) => parts.push(value.clone()),
        (false, true, _) => parts.push(label.into()),
        (false, false, Cell::Leaf(Content::Text(value))) if !tight => {
            let mut spans = label;
            push(&mut spans, " ", &Style::default());
            spans.extend(value.iter().cloned());
            parts.push(spans.into());
        }
        (false, false, _) => parts.extend([label.into(), value.clone()]),
    }
    let mut lines = Vec::new();
    let mut ends_in_box = None;
    for part in parts {
        // Text keeps the stacked layout, whatever the rows of a box.
        let (part_lines, part_ends) = match &part {
            Cell::Leaf(Content::Text(spans)) => (wrap(spans, max), false),
            part => part.drawn(max, styles),
        };
        if !part_lines.is_empty() {
            ends_in_box = Some(part_ends);
        }
        lines.extend(part_lines);
    }
    (lines, ends_in_box)
}

/// How many lines each body row of a table takes when it is drawn as a part
/// of the table, as [`drawn_part`] draws it with the same widths, or
/// stacked without them: found without drawing a cell that is plain text
/// that fits, so that a long table is measured quickly.
pub struct Heights<'a> {
    widths: Option<&'a [usize]>,
    max: usize,
    styles: &'a Styles,
    /// For a stacked table, each column's label, the cells it takes and the
    /// lines it takes alone.
    labels: Vec<(Vec<Span>, usize, usize)>,
    /// The text of each label, when it is plain text.
    plain: Vec<Option<String>>,
    /// Whether the values of a stacked table go under their labels.
    tight: bool,
}

/// A cell of a table to measure: its text, when it is that alone in the
/// style of the body, or the spans the layout makes of it.
pub enum CellText<'t> {
    Text(&'t str),
    Spans(Vec<Span>),
}

impl<'a> Heights<'a> {
    /// The heights of the rows of a table whose header row is `header`, of
    /// `columns` columns, in `max` cells with the column `widths`, or
    /// stacked without them.
    pub fn new(
        header: &[Cell<Content>],
        columns: usize,
        widths: Option<&'a [usize]>,
        max: usize,
        styles: &'a Styles,
    ) -> Self {
        let header = header.to_vec();
        let labels: Vec<(Vec<Span>, usize, usize)> = match widths {
            Some(_) => Vec::new(),
            None => (labels(Some(&header), columns, styles).into_iter())
                .map(|label| {
                    let lines = wrap(&label, max).len();
                    let cells = width(&label);
                    (label, cells, lines)
                })
                .collect(),
        };
        let longest = labels.iter().map(|(_, cells, _)| *cells).max().unwrap_or(0);
        // A label is plain text only when it wraps as its text alone does:
        // its spaces collapse and it is at the normal size. Code keeps its
        // spaces and a superscript is small, so their entries are drawn.
        let plain = (labels.iter())
            .map(|(label, _, _)| {
                let wraps_as_text = label.iter().all(|span| {
                    span.embed.is_none()
                        && span.style.spaces == Spaces::Collapse
                        && span.style.size == TextSize::NORMAL
                });
                let text = style::text(label);
                (wraps_as_text && plain_width(&text).is_some()).then_some(text)
            })
            .collect();
        Self {
            widths,
            max,
            styles,
            labels,
            plain,
            tight: max < longest + 1 + SHORT_WORD,
        }
    }

    /// The lines before the body row numbered `index`: the rule between two
    /// rows, when they have one.
    pub fn before(&self, index: usize) -> usize {
        match self.widths {
            Some(_) => usize::from(self.styles.rows == Rows::Lines && index > 0),
            None => usize::from(index > 0),
        }
    }

    /// The lines that the cell of `column` takes: in its column of a box, or
    /// as its entry of a stacked table.
    pub fn cell(&self, column: usize, cell: CellText) -> usize {
        let Some(widths) = self.widths else {
            return self.entry(column, cell);
        };
        let Some(&max) = widths.get(column) else {
            return 0;
        };
        let plain = match cell {
            CellText::Text(text) => plain_width(text),
            CellText::Spans(_) => None,
        };
        match cell {
            _ if plain.is_some_and(|cells| cells <= max) => 1,
            // Text wraps as `Cell::drawn` has it, and is counted so.
            CellText::Text(text) if plain.is_some() => {
                let lines = plain_rows(text, max);
                match self.styles.rows {
                    Rows::Indent if lines >= 2 => plain_rows(text, max.saturating_sub(2).max(1)),
                    _ => lines,
                }
            }
            cell => Cell::from(cell.spans()).drawn(max, self.styles).0.len(),
        }
    }

    /// The lines of a row whose cells take `cells` lines each, without the
    /// rule before it: the most of them in a box, all of them stacked.
    pub fn row(&self, cells: impl Iterator<Item = usize>) -> usize {
        match self.widths {
            Some(_) => cells.max().unwrap_or(0).max(1),
            None => cells.sum(),
        }
    }

    /// The lines of the entry of `column` in a stacked table.
    fn entry(&self, column: usize, cell: CellText) -> usize {
        let (label, cells, lines) = match self.labels.get(column) {
            Some((label, cells, lines)) => (label.as_slice(), *cells, *lines),
            None => (&[][..], 0, 0),
        };
        let blank = is_empty(label);
        if let CellText::Text(text) = &cell
            && let Some(value) = plain_width(text)
        {
            let rows = |text: &str| plain_rows(text, self.max);
            let label = self.plain.get(column).and_then(Option::as_deref);
            let known = match (blank, text.trim().is_empty()) {
                (true, true) => Some(0),
                (false, true) => Some(lines),
                (true, false) => Some(rows(text)),
                (false, false) if self.tight => Some(lines + rows(text)),
                // With its label on one line, which a label that is not plain
                // text may wrap otherwise than its cells say.
                (false, false) => label.map(|label| match cells + 1 + value <= self.max {
                    true => 1,
                    false => rows(&format!("{label} {text}")),
                }),
            };
            if let Some(known) = known {
                return known;
            }
        }
        let value = Cell::from(cell.spans());
        entry(label.to_vec(), &value, self.tight, self.max, self.styles)
            .0
            .len()
    }
}

impl CellText<'_> {
    /// The spans the layout makes of the cell.
    fn spans(self) -> Vec<Span> {
        match self {
            CellText::Text("") => Vec::new(),
            CellText::Text(text) => vec![Span::new(text, Style::default())],
            CellText::Spans(spans) => spans,
        }
    }
}

/// The widest of the lines a `\n`, as from a `<br>`, splits `spans` into.
fn widest_line(spans: &[Span]) -> usize {
    let (mut widest, mut line) = (0, 0);
    for span in spans {
        if span.embed.is_some() || !span.text.contains('\n') {
            line += width(std::slice::from_ref(span));
            continue;
        }
        for (index, part) in span.text.split('\n').enumerate() {
            if index > 0 {
                widest = widest.max(line);
                line = 0;
            }
            line += span.style.size.width(part);
        }
    }
    widest.max(line)
}

fn is_empty(cell: &[Span]) -> bool {
    cell.iter().all(|span| span.text.trim().is_empty())
}

/// The longest run without spaces, so code is measured by its words even
/// where the theme keeps its spaces together. Never wider than the cell as
/// drawn: smaller text takes fewer cells than it has letters, and no word is
/// wider than its cell.
fn longest_word(cell: &[Span]) -> usize {
    let text = style::text(cell);
    text.split([' ', '\n'])
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
        .min(width(cell))
}

/// The longest text kept together, as the wrapper places it on a line.
fn longest_together(cell: &[Span]) -> usize {
    (words(cell).iter())
        .filter(|word| keeps_together(word))
        .map(|word| width(word))
        .max()
        .unwrap_or(0)
}

/// `spans` wrapped to `max` cells, the lines after the first two cells in.
fn hanging(spans: &[Span], max: usize) -> Vec<Vec<Span>> {
    let lines = wrap(spans, max);
    if lines.len() < 2 {
        return lines;
    }
    let mut lines = wrap(spans, max.saturating_sub(2).max(1));
    for line in lines.iter_mut().skip(1) {
        line.insert(0, Span::new("  ", Style::default()));
    }
    lines
}

/// A rule across the table: its ends, and between them the rule itself.
fn border_line(widths: &[usize], [left, middle, right]: [&str; 3], style: &Style) -> Line {
    let segments: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
    Line::new(vec![
        Span::new(left, style.clone()),
        Span::new(&segments.join(middle), style.clone()),
        Span::new(right, style.clone()),
    ])
}

/// `line` of a table with `background` behind every span without a
/// background of its own, but for its first and last, the table's own
/// borders: a background behind them would stick out half a cell past the
/// lines they draw.
fn painted(mut line: Line, background: Option<Color>) -> Line {
    if background.is_some() {
        let inside = 1..line.spans.len().saturating_sub(1);
        for span in line.spans.get_mut(inside).into_iter().flatten() {
            span.style.bg = span.style.bg.or(background);
        }
    }
    line
}

/// The spans of the last line of a row, laid out as `wrapped` and joined
/// by `join_row`, that belong to a column whose last line is the bottom
/// border of a table drawn as a box, as `ends` says for each column. That
/// border already closes the row in its column, so an underline there
/// would double it.
fn boxed_spans(wrapped: &[Vec<Vec<Span>>], ends: &[bool]) -> Vec<Range<usize>> {
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(0);
    // After the left border, each column is its padding, its text, its
    // padding and the border after it.
    let mut start = 1;
    let mut boxed = Vec::new();
    for (column, lines) in wrapped.iter().enumerate() {
        let spans = lines.get(height.saturating_sub(1)).map_or(0, Vec::len) + 2;
        if lines.len() == height && ends.get(column) == Some(&true) {
            boxed.push(start..start + spans);
        }
        start += spans + 1;
    }
    boxed
}

/// Underlines `line` between the table's borders, dashed, all of it in
/// `color`, but for the spans in `skip`. Text with an underline of its own,
/// such as a link, keeps its style but takes the colour, so that the row's
/// underline is one colour from border to border.
fn underline(line: &mut Line, color: Option<Color>, skip: &[Range<usize>]) {
    let inside = 1..line.spans.len().saturating_sub(1);
    let spans = (line.spans.iter_mut().enumerate())
        .filter(|(index, _)| inside.contains(index))
        .filter(|(index, _)| !skip.iter().any(|range| range.contains(index)));
    for (_, span) in spans {
        if span.style.underline == Underline::None {
            span.style.underline = Underline::Dashed;
        }
        span.style.underline_color = color;
    }
}

/// The lines of `row`, on `background` from border to border.
fn row_lines(
    row: &[Cell<Content>],
    widths: &[usize],
    alignments: &[Alignment],
    styles: &Styles,
    background: Option<Color>,
) -> Vec<Line> {
    let (wrapped, _) = wrap_row(row, widths, styles);
    join_row(&wrapped, widths, alignments, styles, background)
}

/// The lines of each cell of `row` in its column's width, and for each
/// whether its last line is the bottom border of a box.
fn wrap_row(
    row: &[Cell<Content>],
    widths: &[usize],
    styles: &Styles,
) -> (Vec<Vec<Vec<Span>>>, Vec<bool>) {
    (widths.iter().enumerate())
        .map(|(column, &w)| match row.get(column) {
            Some(cell) => cell.drawn(w, styles),
            None => (vec![Vec::new()], false),
        })
        .unzip()
}

/// The cells' lines side by side between borders, on `background`.
fn join_row(
    wrapped: &[Vec<Vec<Span>>],
    widths: &[usize],
    alignments: &[Alignment],
    styles: &Styles,
    background: Option<Color>,
) -> Vec<Line> {
    let border = &styles.border;
    let empty = Vec::new();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);

    (0..height)
        .map(|line_index| {
            let mut spans = vec![Span::new("│", border.clone())];
            for (column, cell_lines) in wrapped.iter().enumerate() {
                let content = cell_lines.get(line_index).unwrap_or(&empty);
                let alignment = alignments.get(column).copied().unwrap_or_default();
                let free = widths[column].saturating_sub(width(content));
                let (before, after) = match alignment {
                    Alignment::Left => (0, free),
                    Alignment::Right => (free, 0),
                    Alignment::Center => (free / 2, free - free / 2),
                };
                spans.push(Span::new(&" ".repeat(before + 1), Style::default()));
                spans.extend(content.iter().cloned());
                spans.push(Span::new(&" ".repeat(after + 1), Style::default()));
                spans.push(Span::new("│", border.clone()));
            }
            painted(Line::new(spans), background)
        })
        .collect()
}

#[cfg(test)]
mod tests;
