//! Tables drawn with box-drawing characters, or stacked when too narrow.

use std::ops::Range;

use unicode_width::UnicodeWidthStr;

use crate::typeset::color::{Color, Underline};
use crate::typeset::model::Alignment;
use crate::typeset::style::{self, Line, Span, Style, push};
use crate::typeset::wrap::{keeps_together, split, width, words, wrap};

/// Cells as styled spans; rows may be shorter than the header.
#[derive(Debug, Clone, PartialEq)]
pub struct Cells {
    pub header: Vec<Vec<Span>>,
    pub rows: Vec<Vec<Cell>>,
    pub alignments: Vec<Alignment>,
    /// The first column holds the headers, styled as the caller styles
    /// them, in place of a header row, which is then left out.
    pub header_column: bool,
}

/// What a cell holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    /// Text, wrapped to the column.
    Text(Vec<Span>),
    /// A table, laid out in the column's width.
    Table(Cells),
    /// Text and tables one under another.
    Stack(Vec<Cell>),
    /// Lines of code, kept as written and split where they are too long,
    /// each filled to the column's width in `fill`, as a code block is.
    Code { lines: Vec<Vec<Span>>, fill: Style },
}

impl From<Vec<Span>> for Cell {
    fn from(spans: Vec<Span>) -> Self {
        Self::Text(spans)
    }
}

impl Cell {
    /// Cells it takes without wrapping.
    fn natural(&self) -> usize {
        match self {
            Self::Text(spans) => width(spans),
            Self::Table(cells) => natural_width(cells),
            Self::Stack(parts) => parts.iter().map(Cell::natural).max().unwrap_or(0),
            Self::Code { lines, .. } => code_width(lines),
        }
    }

    /// The fewest cells it takes without splitting a short word. A table
    /// that narrow is stacked.
    fn floor(&self) -> usize {
        match self {
            Self::Text(spans) => longest_word(spans).min(SHORT_WORD),
            Self::Table(cells) => (floors(cells, column_count(cells)).into_iter())
                .max()
                .unwrap_or(0),
            Self::Stack(parts) => parts.iter().map(Cell::floor).max().unwrap_or(0),
            Self::Code { lines, .. } => code_width(lines).min(SHORT_WORD),
        }
    }

    /// The cells it would rather keep on one line: text kept together, or
    /// a table as a box.
    fn together(&self) -> usize {
        match self {
            Self::Text(spans) => longest_together(spans),
            Self::Table(cells) => boxed_floor(cells),
            Self::Stack(parts) => parts.iter().map(Cell::together).max().unwrap_or(0),
            // Code split at the edge reads badly.
            Self::Code { lines, .. } => code_width(lines),
        }
    }

    /// Its lines in `max` cells, and whether the last of them is the bottom
    /// border of a table drawn as a box.
    fn drawn(&self, max: usize, styles: &Styles) -> (Vec<Vec<Span>>, bool) {
        match self {
            Self::Text(spans) if styles.rows == Rows::Indent => (hanging(spans, max), false),
            Self::Text(spans) => (wrap(spans, max), false),
            Self::Table(cells) => {
                let (lines, boxed) = drawn_table(cells, max, styles);
                (lines.into_iter().map(|line| line.spans).collect(), boxed)
            }
            Self::Stack(parts) => stack_lines(parts, max, styles),
            Self::Code { lines, fill } => {
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
            Self::Text(spans) => is_empty(spans),
            Self::Table(cells) => cells.rows.is_empty(),
            Self::Stack(parts) => parts.iter().all(Cell::is_empty),
            Self::Code { lines, .. } => lines.iter().all(|line| is_empty(line)),
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
fn stack_lines(parts: &[Cell], max: usize, styles: &Styles) -> (Vec<Vec<Span>>, bool) {
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
                let (stacked, ends) = stacked(cells, column_count(cells), max, styles);
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
fn stack_is_boxed(parts: &[Cell], max: usize) -> bool {
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
pub fn layout(cells: &Cells, max: usize, styles: &Styles) -> Vec<Line> {
    drawn_table(cells, max, styles).0
}

/// `layout`'s lines, and whether the last of them is the bottom border of
/// a box: the table's own, or one in its last value when it is stacked.
fn drawn_table(cells: &Cells, max: usize, styles: &Styles) -> (Vec<Line>, bool) {
    let border = &styles.border;
    let columns = column_count(cells);
    if columns == 0 {
        return (Vec::new(), false);
    }
    let Some(widths) = column_widths(cells, columns, max) else {
        return stacked(cells, columns, max, styles);
    };

    let base = styles.base;
    let rule = |ends| painted(border_line(&widths, ends, border), base);
    // The top and bottom borders are not painted: a rule is drawn through
    // the middle of its cells, so `base` behind them would show half a
    // cell outside the box.
    let edge = |ends| border_line(&widths, ends, border);
    let mut lines = vec![edge(["╭", "┬", "╮"])];
    if !cells.header_column {
        let header: Vec<Cell> = cells.header.iter().cloned().map(Cell::Text).collect();
        lines.extend(row_lines(&header, &widths, &cells.alignments, styles, base));
        lines.push(rule(["├", "┼", "┤"]));
    }
    for (index, row) in cells.rows.iter().enumerate() {
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
                    &widths,
                    &cells.alignments,
                    &styles,
                    Some(stripe),
                ));
                continue;
            }
            Rows::Underline { color } if index + 1 < cells.rows.len() => {
                let (wrapped, ends) = wrap_row(row, &widths, styles);
                let mut row_lines = join_row(&wrapped, &widths, &cells.alignments, styles, base);
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
            &widths,
            &cells.alignments,
            styles,
            background,
        ));
    }
    lines.push(edge(["╰", "┴", "╯"]));
    (lines, true)
}

fn column_count(cells: &Cells) -> usize {
    (cells.rows.iter())
        .map(Vec::len)
        .chain([cells.header.len(), cells.alignments.len()])
        .max()
        .unwrap_or(0)
}

/// Each column's widest cell by `measure`.
fn column_maxima(cells: &Cells, columns: usize, measure: impl Fn(&Cell) -> usize) -> Vec<usize> {
    let mut maxima = vec![0; columns];
    let header = (!cells.header_column).then(|| {
        (cells.header.iter())
            .map(|header| Cell::Text(header.clone()))
            .collect::<Vec<_>>()
    });
    for row in header.iter().chain(&cells.rows) {
        for (column, cell) in row.iter().enumerate() {
            maxima[column] = maxima[column].max(measure(cell));
        }
    }
    maxima
}

/// The fewest cells each column takes without splitting a short word, or
/// any word of a header column.
fn floors(cells: &Cells, columns: usize) -> Vec<usize> {
    let mut floors = column_maxima(cells, columns, Cell::floor);
    if cells.header_column {
        for row in &cells.rows {
            if let Some(Cell::Text(header)) = row.first() {
                floors[0] = floors[0].max(longest_word(header));
            }
        }
    }
    floors.into_iter().map(|floor| floor.max(1)).collect()
}

/// The width of the table drawn as a box without wrapping.
fn natural_width(cells: &Cells) -> usize {
    let columns = column_count(cells);
    let widths = column_maxima(cells, columns, Cell::natural);
    widths.iter().map(|width| width.max(&1)).sum::<usize>() + 3 * columns + 1
}

/// The fewest cells the table takes as a box.
fn boxed_floor(cells: &Cells) -> usize {
    let columns = column_count(cells);
    floors(cells, columns).iter().sum::<usize>() + 3 * columns + 1
}

/// Widths that fit in `max` without splitting short words, or `None`.
fn column_widths(cells: &Cells, columns: usize, max: usize) -> Option<Vec<usize>> {
    // No column is narrower than its longest short word. That covers its
    // widest character too, two cells for CJK and emoji, which would push
    // its borders out of line.
    let at_least_one = |widths: Vec<usize>| widths.into_iter().map(|w| w.max(1)).collect();
    let mut widths: Vec<usize> = at_least_one(column_maxima(cells, columns, Cell::natural));
    let minimums = floors(cells, columns);
    let together = column_maxima(cells, columns, Cell::together);
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
    let fits = shrink(&mut widths, &minimums, available);
    debug_assert!(fits, "the minimums fit");
    Some(widths)
}

/// Narrows the widths to fit in `available`, as a browser's automatic
/// table layout does: every column keeps its floor, and the room above
/// the floors is shared in proportion to how much each column wants above
/// its own. The cells left over by rounding down go to the largest
/// remainders, the leftmost column first on a tie. False if the floors do
/// not fit.
fn shrink(widths: &mut [usize], floors: &[usize], available: usize) -> bool {
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
fn stacked(cells: &Cells, columns: usize, max: usize, styles: &Styles) -> (Vec<Line>, bool) {
    let label = |header: &[Span]| {
        let mut label = header.to_vec();
        if !is_empty(header) {
            push(&mut label, ":", &styles.header);
        }
        label
    };
    let empty = Cell::Text(Vec::new());
    // A table with a header only still shows its headers.
    let no_row = [Vec::new()];
    let rows: &[Vec<Cell>] = if cells.rows.is_empty() && !cells.header_column {
        &no_row
    } else {
        &cells.rows
    };
    // Each row as its headers and its values.
    let entries: Vec<Vec<(Vec<Span>, &Cell)>> = if cells.header_column {
        (rows.iter())
            .map(|row| {
                let header = match row.first() {
                    Some(Cell::Text(header)) => label(header),
                    _ => Vec::new(),
                };
                let values = row.iter().skip(1).chain(std::iter::repeat(&empty));
                let headers = std::iter::once(header).chain(std::iter::repeat(Vec::new()));
                headers.zip(values).take(columns.max(2) - 1).collect()
            })
            .collect()
    } else {
        let labels: Vec<Vec<Span>> = (0..columns)
            .map(|column| label(cells.header.get(column).map_or(&[], Vec::as_slice)))
            .collect();
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
    for (index, row) in entries.into_iter().enumerate() {
        if index > 0 && !cells.header_column {
            lines.push(Line::new(vec![Span::new(
                &"─".repeat(max),
                styles.border.clone(),
            )]));
            ends_in_box = false;
        }
        for (label, value) in row {
            let mut parts: Vec<Cell> = Vec::new();
            match (is_empty(&label), value.is_empty(), value) {
                (true, true, _) => {}
                (true, false, _) => parts.push(value.clone()),
                (false, true, _) => parts.push(Cell::Text(label)),
                (false, false, Cell::Text(value)) if !tight => {
                    let mut spans = label;
                    push(&mut spans, " ", &Style::default());
                    spans.extend(value.iter().cloned());
                    parts.push(Cell::Text(spans));
                }
                (false, false, _) => parts.extend([Cell::Text(label), value.clone()]),
            }
            for part in parts {
                // Text keeps the stacked layout, whatever the rows of a box.
                let (part_lines, part_ends) = match &part {
                    Cell::Text(spans) => (wrap(spans, max), false),
                    part => part.drawn(max, styles),
                };
                if !part_lines.is_empty() {
                    ends_in_box = part_ends;
                }
                lines.extend(part_lines.into_iter().map(Line::new));
            }
        }
    }
    (lines, ends_in_box)
}

fn is_empty(cell: &[Span]) -> bool {
    cell.iter().all(|span| span.text.trim().is_empty())
}

/// The longest run without spaces, so code is measured by its words even
/// where the theme keeps its spaces together.
fn longest_word(cell: &[Span]) -> usize {
    let text = style::text(cell);
    text.split([' ', '\n'])
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
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
    row: &[Cell],
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
fn wrap_row(row: &[Cell], widths: &[usize], styles: &Styles) -> (Vec<Vec<Vec<Span>>>, Vec<bool>) {
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
