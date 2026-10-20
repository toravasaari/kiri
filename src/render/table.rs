//! Tables drawn with box-drawing characters, or stacked when too narrow.

use std::ops::Range;

use unicode_width::UnicodeWidthStr;

use super::style::{self, Line, Span, Style, push};
use super::wrap::{keeps_together, split, width, words, wrap};
use crate::color::{Color, Underline};
use crate::model::Alignment;

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
mod tests {
    use super::*;
    use crate::render::style::Spaces;

    fn cell(text: &str) -> Vec<Span> {
        vec![Span::new(text, Style::default())]
    }

    fn styles() -> Styles {
        Styles {
            border: Style::default(),
            header: Style::default(),
            rows: Rows::Plain,
            base: None,
        }
    }

    /// `styles` with `rows`.
    fn with_rows(rows: Rows) -> Styles {
        Styles { rows, ..styles() }
    }

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|line| style::text(&line.spans)).collect()
    }

    fn cell_text(lines: &[Vec<Span>]) -> Vec<String> {
        lines.iter().map(|line| style::text(line)).collect()
    }

    #[test]
    fn aligns_cells_within_columns() {
        let cells = Cells {
            header: vec![cell("left"), cell("right")],
            rows: vec![vec![cell("a").into(), cell("b").into()]],
            alignments: vec![Alignment::Left, Alignment::Right],
            header_column: false,
        };
        assert_eq!(
            text(&layout(&cells, 80, &styles())),
            vec![
                "╭──────┬───────╮",
                "│ left │ right │",
                "├──────┼───────┤",
                "│ a    │     b │",
                "╰──────┴───────╯",
            ]
        );
    }

    #[test]
    fn shares_the_width_in_proportion_to_the_text_and_wraps_it() {
        // Natural widths 59 and 29 over floors of 16 want 43 and 13 cells
        // more; the 18 cells of room are shared 14 and 4.
        let long = ["aaaa"; 12].join(" ");
        let short = ["bbbb"; 6].join(" ");
        let cells = table(&["Long", "Short"], &[&[&long, &short]]);
        let lines = text(&layout(&cells, 57, &styles()));
        assert!(lines.iter().all(|line| line.width() == 57), "{lines:#?}");
        assert_eq!(lines[0], format!("╭{}┬{}╮", "─".repeat(32), "─".repeat(22)));
        assert_eq!(
            lines[3],
            "│ aaaa aaaa aaaa aaaa aaaa aaaa  │ bbbb bbbb bbbb bbbb  │"
        );
        assert_eq!(
            lines[4],
            "│ aaaa aaaa aaaa aaaa aaaa aaaa  │ bbbb bbbb            │"
        );
    }

    #[test]
    fn rounding_fills_the_room_exactly_leftmost_first() {
        // Ten cells of room for three columns that want the same: 3 each,
        // and the cell left over goes to the first.
        let mut widths = vec![20, 20, 20];
        assert!(shrink(&mut widths, &[5, 5, 5], 25));
        assert_eq!(widths, [9, 8, 8]);
        // The larger remainder wins over the column order.
        let mut widths = vec![9, 13];
        assert!(shrink(&mut widths, &[1, 1], 8));
        assert_eq!(widths, [3, 5]);
        assert!(!shrink(&mut widths, &[4, 4], 7));
    }

    #[test]
    fn a_column_at_its_floor_keeps_its_width_while_others_shrink() {
        // The first column wants nothing above its floor; the others share
        // the 11 cells of room 30:20, 6.6 and 4.4 rounded to 7 and 4.
        let mut widths = vec![6, 40, 30];
        assert!(shrink(&mut widths, &[6, 10, 10], 37));
        assert_eq!(widths, [6, 17, 14]);
    }

    #[test]
    fn an_odd_width_column_of_wide_characters_keeps_its_borders_in_line() {
        // A word of twelve cells may split; at eleven cells, five of its
        // characters fit and the borders stay in line.
        let cells = Cells {
            header: vec![cell("名前名前名前"), cell("ab")],
            rows: vec![vec![cell("東京").into(), cell("🦀").into()]],
            alignments: vec![],
            header_column: false,
        };
        let lines = text(&layout(&cells, 20, &styles()));
        let widths: Vec<usize> = lines.iter().map(|line| line.width()).collect();
        assert!(widths.iter().all(|&w| w == 20), "{lines:#?}");
        assert_eq!(lines[1], "│ 名前名前名  │ ab │");
        assert_eq!(lines[2], "│ 前          │    │");
    }

    #[test]
    fn fills_missing_cells() {
        let cells = Cells {
            header: vec![cell("a"), cell("b")],
            rows: vec![vec![cell("1").into()]],
            alignments: vec![],
            header_column: false,
        };
        assert_eq!(text(&layout(&cells, 80, &styles()))[3], "│ 1 │   │");
    }

    fn table(header: &[&str], rows: &[&[&str]]) -> Cells {
        Cells {
            header: header.iter().map(|text| cell(text)).collect(),
            rows: (rows.iter())
                .map(|row| row.iter().map(|text| cell(text).into()).collect())
                .collect(),
            alignments: vec![],
            header_column: false,
        }
    }

    #[test]
    fn narrows_other_columns_before_splitting_a_short_word() {
        let cells = table(
            &["Name", "Notes"],
            &[&["Wonderful things happen", "x y z w v u t s r q p"]],
        );
        let lines = text(&layout(&cells, 21, &styles()));
        assert!(lines.iter().all(|line| line.width() <= 21), "{lines:#?}");
        assert_eq!(lines[0], "╭───────────┬───────╮");
        assert_eq!(lines[3], "│ Wonderful │ x y z │");
    }

    #[test]
    fn a_long_url_alone_does_not_stack_the_table() {
        let cells = table(&["Link"], &[&["https://example.com/a/very/long/path"]]);
        let lines = text(&layout(&cells, 20, &styles()));
        assert!(lines.iter().all(|line| line.width() <= 20), "{lines:#?}");
        assert_eq!(lines[3], "│ https://example. │");
    }

    fn code(text: &str, spaces: Spaces) -> Vec<Span> {
        let style = Style {
            spaces,
            ..Style::default()
        };
        vec![Span::new(text, style)]
    }

    fn command_table(spaces: Spaces) -> Cells {
        Cells {
            header: vec![cell("Notes"), cell("Command")],
            rows: vec![vec![
                cell("a fairly long note about the step").into(),
                code("git commit -m message", spaces).into(),
            ]],
            alignments: vec![],
            header_column: false,
        }
    }

    #[test]
    fn code_wraps_at_its_spaces_like_text() {
        // Its longest word is seven cells, so the table fits as a box.
        let lines = text(&layout(&command_table(Spaces::Keep), 20, &styles()));
        assert_eq!(lines[0], "╭────────┬─────────╮");
        assert_eq!(lines[3], "│ a      │ git     │");
        assert_eq!(lines[4], "│ fairly │ commit  │");
    }

    #[test]
    fn code_kept_together_stays_whole_while_other_columns_keep_room() {
        let cells = command_table(Spaces::KeepTogether);
        let lines = text(&layout(&cells, 44, &styles()));
        assert!(lines.iter().all(|line| line.width() <= 44), "{lines:#?}");
        assert_eq!(lines[3], "│ a fairly long    │ git commit -m message │");
        // The same table wraps the code at its spaces when its spaces are
        // ordinary break points.
        let lines = text(&layout(&command_table(Spaces::Keep), 44, &styles()));
        assert_eq!(lines[3], "│ a fairly long note   │ git commit -m     │");
    }

    #[test]
    fn code_kept_together_does_not_squeeze_the_other_columns() {
        // Too narrow for the code and a comfortable description: the table
        // is laid out as if the code wrapped at its spaces.
        let together = command_table(Spaces::KeepTogether);
        let wrapping = command_table(Spaces::Keep);
        let lines = text(&layout(&together, 38, &styles()));
        assert_eq!(lines[3], "│ a fairly long note │ git commit -m │");
        assert_eq!(lines, text(&layout(&wrapping, 38, &styles())));
    }

    #[test]
    fn code_kept_together_splits_at_its_spaces_before_the_table_stacks() {
        let cells = command_table(Spaces::KeepTogether);
        let lines = text(&layout(&cells, 24, &styles()));
        assert!(lines.iter().all(|line| line.width() <= 24), "{lines:#?}");
        assert_eq!(lines[3], "│ a fairly  │ git      │");
        assert_eq!(lines[4], "│ long note │ commit   │");
        // Narrower than its longest words, the table stacks.
        let lines = text(&layout(&cells, 19, &styles()));
        assert_eq!(lines[0], "Notes: a fairly");
    }

    #[test]
    fn a_long_url_beside_code_kept_together_may_still_split() {
        let cells = Cells {
            header: vec![cell("Link"), cell("Command")],
            rows: vec![vec![
                cell("https://example.com/a/very/long/path").into(),
                code("git commit", Spaces::KeepTogether).into(),
            ]],
            alignments: vec![],
            header_column: false,
        };
        let lines = text(&layout(&cells, 33, &styles()));
        assert!(lines.iter().all(|line| line.width() <= 33), "{lines:#?}");
        assert_eq!(lines[3], "│ https://example. │ git commit │");
    }

    #[test]
    fn stacks_a_table_whose_short_words_do_not_fit() {
        let cells = table(
            &["Name", "Notes"],
            &[
                &["Helsinki", "Capital of Finland since 1812"],
                &["Tampere", "Inland"],
            ],
        );
        assert_eq!(
            text(&layout(&cells, 20, &styles())),
            vec![
                "Name: Helsinki",
                "Notes: Capital of",
                "Finland since 1812",
                "────────────────────",
                "Name: Tampere",
                "Notes: Inland",
            ]
        );
    }

    #[test]
    fn puts_values_under_headers_that_leave_too_little_room() {
        let cells = table(&["Population", "Capital"], &[&["5600000", "Helsinki"]]);
        assert_eq!(
            text(&layout(&cells, 16, &styles())),
            vec!["Population:", "5600000", "Capital:", "Helsinki"]
        );
    }

    #[test]
    fn stacked_cells_without_a_header_or_a_value() {
        let cells = table(&["", "City", "Notes"], &[&["10th", "Helsinki", ""]]);
        assert_eq!(
            text(&layout(&cells, 18, &styles())),
            vec!["10th", "City: Helsinki", "Notes:"]
        );
        // A row longer than the header, and a cell empty on both sides.
        let cells = table(&["City", ""], &[&["Helsinki", "", "Finland"]]);
        assert_eq!(
            text(&layout(&cells, 18, &styles())),
            vec!["City: Helsinki", "Finland"]
        );
        // A table with a header only shows its headers.
        let cells = table(&["Population", "Capital"], &[]);
        assert_eq!(
            text(&layout(&cells, 16, &styles())),
            vec!["Population:", "Capital:"]
        );
    }

    #[test]
    fn stacked_cells_keep_their_styles() {
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let code = Style {
            italic: true,
            ..Style::default()
        };
        let border = Style {
            dim: true,
            ..Style::default()
        };
        let cells = Cells {
            header: vec![
                vec![Span::new("Name", bold.clone())],
                vec![Span::new("Notes", bold.clone())],
            ],
            rows: vec![
                vec![
                    cell("Helsinki").into(),
                    vec![Span::new("capital", code.clone())].into(),
                ],
                vec![cell("Tampere").into(), cell("inland").into()],
            ],
            alignments: vec![],
            header_column: false,
        };
        let styles = Styles {
            border: border.clone(),
            header: bold.clone(),
            ..styles()
        };
        let lines = layout(&cells, 20, &styles);
        assert_eq!(
            lines[1].spans,
            vec![
                Span::new("Notes:", bold),
                Span::new(" ", Style::default()),
                Span::new("capital", code),
            ]
        );
        assert_eq!(lines[2].spans, vec![Span::new(&"─".repeat(20), border)]);
    }

    /// A table of `keys` in a header column and their values.
    fn keyed(entries: Vec<(&str, Cell)>) -> Cells {
        Cells {
            header: Vec::new(),
            rows: (entries.into_iter())
                .map(|(key, value)| vec![cell(key).into(), value])
                .collect(),
            alignments: Vec::new(),
            header_column: true,
        }
    }

    #[test]
    fn a_header_column_takes_the_place_of_the_header_row() {
        let cells = keyed(vec![
            ("name", cell("kiri").into()),
            ("tags", cell("docs, terminal").into()),
        ]);
        assert_eq!(
            text(&layout(&cells, 80, &with_rows(Rows::Lines))),
            vec![
                "╭──────┬────────────────╮",
                "│ name │ kiri           │",
                "├──────┼────────────────┤",
                "│ tags │ docs, terminal │",
                "╰──────┴────────────────╯",
            ]
        );
    }

    #[test]
    fn a_table_in_a_cell_is_laid_out_in_its_column() {
        let author = keyed(vec![
            ("name", cell("Aino").into()),
            ("site", cell("example.com").into()),
        ]);
        let cells = keyed(vec![
            ("title", cell("Notes on a long and winding road").into()),
            ("author", Cell::Table(author)),
        ]);
        assert_eq!(
            text(&layout(&cells, 80, &with_rows(Rows::Lines))),
            vec![
                "╭────────┬──────────────────────────────────╮",
                "│ title  │ Notes on a long and winding road │",
                "├────────┼──────────────────────────────────┤",
                "│ author │ ╭──────┬─────────────╮           │",
                "│        │ │ name │ Aino        │           │",
                "│        │ ├──────┼─────────────┤           │",
                "│        │ │ site │ example.com │           │",
                "│        │ ╰──────┴─────────────╯           │",
                "╰────────┴──────────────────────────────────╯",
            ]
        );
        // Narrower, the text wraps while the table in the cell keeps its
        // box.
        let lines = text(&layout(&cells, 36, &with_rows(Rows::Lines)));
        assert!(lines.iter().all(|line| line.width() <= 36), "{lines:#?}");
        assert_eq!(
            lines,
            vec![
                "╭────────┬─────────────────────────╮",
                "│ title  │ Notes on a long and     │",
                "│        │ winding road            │",
                "├────────┼─────────────────────────┤",
                "│ author │ ╭──────┬─────────────╮  │",
                "│        │ │ name │ Aino        │  │",
                "│        │ ├──────┼─────────────┤  │",
                "│        │ │ site │ example.com │  │",
                "│        │ ╰──────┴─────────────╯  │",
                "╰────────┴─────────────────────────╯",
            ]
        );
    }

    #[test]
    fn a_table_in_a_cell_stacks_before_the_table_around_it() {
        let author = keyed(vec![
            ("name", cell("Aino").into()),
            ("site", cell("example.com").into()),
        ]);
        let cells = keyed(vec![("author", Cell::Table(author))]);
        assert_eq!(
            text(&layout(&cells, 28, &styles())),
            vec![
                "╭────────┬─────────────────╮",
                "│ author │ name:           │",
                "│        │ Aino            │",
                "│        │ site:           │",
                "│        │ example.com     │",
                "╰────────┴─────────────────╯",
            ]
        );
    }

    #[test]
    fn a_stack_puts_its_parts_one_under_another() {
        let item = |title: &str| {
            Cell::Table(keyed(vec![
                ("title", cell(title).into()),
                ("icon", cell("⚡").into()),
            ]))
        };
        let features = Cell::Stack(vec![item("Fast"), cell("and").into(), item("Small")]);
        let cells = keyed(vec![("features", features)]);
        assert_eq!(
            text(&layout(&cells, 80, &with_rows(Rows::Lines))),
            vec![
                "╭──────────┬───────────────────╮",
                "│ features │ ╭───────┬──────╮  │",
                "│          │ │ title │ Fast │  │",
                "│          │ ├───────┼──────┤  │",
                "│          │ │ icon  │ ⚡   │  │",
                "│          │ ╰───────┴──────╯  │",
                "│          │ and               │",
                "│          │ ╭───────┬───────╮ │",
                "│          │ │ title │ Small │ │",
                "│          │ ├───────┼───────┤ │",
                "│          │ │ icon  │ ⚡    │ │",
                "│          │ ╰───────┴───────╯ │",
                "╰──────────┴───────────────────╯",
            ]
        );
    }

    #[test]
    fn a_table_with_a_header_column_stacks_without_rules_between_rows() {
        let author = keyed(vec![("name", cell("Aino").into())]);
        let cells = keyed(vec![
            ("title", cell("Notes").into()),
            ("description", cell("A rather long description").into()),
            ("author", Cell::Table(author)),
        ]);
        assert_eq!(
            text(&layout(&cells, 24, &styles())),
            vec![
                "title: Notes",
                "description: A rather",
                "long description",
                "author:",
                "╭──────┬──────╮",
                "│ name │ Aino │",
                "╰──────┴──────╯",
            ]
        );
    }

    #[test]
    fn words_in_a_header_column_stay_whole() {
        let cells = keyed(vec![
            ("description", cell("How the build is set up").into()),
            ("robots", cell("").into()),
        ]);
        assert_eq!(
            text(&layout(&cells, 28, &with_rows(Rows::Lines))),
            vec![
                "╭─────────────┬────────────╮",
                "│ description │ How the    │",
                "│             │ build is   │",
                "│             │ set up     │",
                "├─────────────┼────────────┤",
                "│ robots      │            │",
                "╰─────────────┴────────────╯",
            ]
        );
    }

    #[test]
    fn tables_in_a_stack_stack_together() {
        let stop = |name: &str, km: &str| {
            Cell::Table(keyed(vec![
                ("name", cell(name).into()),
                ("km", cell(km).into()),
            ]))
        };
        // The second table needs 21 cells as a box, the first 18.
        let stops = Cell::Stack(vec![
            stop("Harbour", "0"),
            stop("Lighthouse", "7.5"),
            cell("More to come").into(),
        ]);
        assert_eq!(
            cell_text(&stops.drawn(18, &with_rows(Rows::Lines)).0),
            vec![
                "name: Harbour",
                "km: 0",
                "──────────────────",
                "name: Lighthouse",
                "km: 7.5",
                "──────────────────",
                "More to come",
            ]
        );
        assert_eq!(
            cell_text(&stops.drawn(21, &with_rows(Rows::Lines)).0)[5],
            "╭──────┬────────────╮"
        );
    }

    #[test]
    fn code_in_a_cell_keeps_its_spaces_and_fills_the_column() {
        let fill = Style {
            italic: true,
            ..Style::default()
        };
        let code = Cell::Code {
            lines: vec![
                cell("flags:"),
                cell("  opt: 3"),
                cell("  name: a-long-name"),
            ],
            fill: fill.clone(),
        };
        let lines = code.drawn(12, &styles()).0;
        assert_eq!(
            cell_text(&lines),
            vec![
                "flags:      ",
                "  opt: 3    ",
                "  name: a-lo",
                "ng-name     "
            ]
        );
        assert_eq!(lines[0][1], Span::new("      ", fill));
        // It is as wide as its longest line, and would rather not split.
        let cells = keyed(vec![("linux", code)]);
        assert_eq!(
            text(&layout(&cells, 80, &styles()))[1],
            "│ linux │ flags:              │"
        );
    }

    #[test]
    fn lines_go_between_the_body_rows_of_any_table() {
        let cells = table(
            &["City", "Notes"],
            &[&["Helsinki", "Capital"], &["Oulu", "North"]],
        );
        assert_eq!(
            text(&layout(&cells, 80, &with_rows(Rows::Lines))),
            vec![
                "╭──────────┬─────────╮",
                "│ City     │ Notes   │",
                "├──────────┼─────────┤",
                "│ Helsinki │ Capital │",
                "├──────────┼─────────┤",
                "│ Oulu     │ North   │",
                "╰──────────┴─────────╯",
            ]
        );
        // Without them, a header column has no rules either.
        let cells = keyed(vec![("a", cell("1").into()), ("b", cell("2").into())]);
        assert_eq!(
            text(&layout(&cells, 80, &styles())),
            vec!["╭───┬───╮", "│ a │ 1 │", "│ b │ 2 │", "╰───┴───╯"]
        );
    }

    const STRIPE: Color = Color::Rgb(1, 1, 1);
    const PAGE: Color = Color::Rgb(0, 0, 0);

    fn stripes() -> Styles {
        with_rows(Rows::Stripes {
            stripe: STRIPE,
            page: PAGE,
        })
    }

    /// The background of each span of `line`.
    fn backgrounds(line: &Line) -> Vec<Option<Color>> {
        line.spans.iter().map(|span| span.style.bg).collect()
    }

    #[test]
    fn stripes_run_across_every_second_body_row() {
        let cells = table(
            &["City", "Notes"],
            &[
                &["Helsinki", "Capital"],
                &["Oulu", "North"],
                &["Turku", "West"],
            ],
        );
        let lines = layout(&cells, 80, &stripes());
        // No rules between the rows.
        assert_eq!(text(&lines).len(), 7);
        for (index, line) in lines.iter().enumerate() {
            let expected = if index == 4 { Some(STRIPE) } else { None };
            // Between the table's own borders, the border between columns
            // included; the outer borders keep the page, as a stripe there
            // would stick out half a cell past the box.
            let bgs = backgrounds(line);
            let (first, last) = (bgs.first().copied(), bgs.last().copied());
            let inside = bgs.get(1..bgs.len().saturating_sub(1)).unwrap_or_default();
            assert_eq!((first, last), (Some(None), Some(None)), "{index}: {line:?}");
            assert!(inside.iter().all(|bg| *bg == expected), "{index}: {line:?}");
        }
    }

    #[test]
    fn stripes_start_afresh_in_a_table_in_a_striped_row() {
        let inner = keyed(vec![("x", cell("1").into()), ("y", cell("2").into())]);
        let cells = keyed(vec![("a", cell("text").into()), ("b", Cell::Table(inner))]);
        let lines = layout(&cells, 80, &stripes());
        assert_eq!(
            text(&lines),
            vec![
                "╭───┬───────────╮",
                "│ a │ text      │",
                "│ b │ ╭───┬───╮ │",
                "│   │ │ x │ 1 │ │",
                "│   │ │ y │ 2 │ │",
                "│   │ ╰───┴───╯ │",
                "╰───┴───────────╯",
            ]
        );
        // The background of each cell of a line.
        let cells = |line: &Line| -> Vec<Option<Color>> {
            (line.spans.iter())
                .flat_map(|span| span.text.chars().map(|_| span.style.bg))
                .collect()
        };
        let (s, p) = (Some(STRIPE), Some(PAGE));
        // The outer row is striped from inside its left border to inside its
        // right one, the inner table's own borders included; inside them
        // the inner table is drawn on the page, its second row striped.
        assert_eq!(
            cells(&lines[3]),
            [None, s, s, s, s, s, s, p, p, p, p, p, p, p, s, s, None]
        );
        assert_eq!(
            cells(&lines[4]),
            [None, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, None]
        );
        // The inner table's top and bottom borders are on the stripe: its
        // rules are drawn through the middle of their cells, and the page
        // colour behind them would show half a cell outside its box.
        assert_eq!(
            cells(&lines[2]),
            [None, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, None]
        );
        assert_eq!(
            cells(&lines[5]),
            [None, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, None]
        );
        assert!(cells(&lines[1]).iter().all(Option::is_none));
    }

    #[test]
    fn indent_starts_the_lines_after_the_first_of_a_cell_two_cells_in() {
        let author = keyed(vec![("name", cell("Aino Virtanen").into())]);
        let cells = keyed(vec![
            (
                "description",
                cell("Drafts notes from merged pull requests").into(),
            ),
            ("notes", cell("one\ntwo").into()),
            ("tags", cell("a, b").into()),
            ("author", Cell::Table(author)),
        ]);
        assert_eq!(
            text(&layout(&cells, 40, &with_rows(Rows::Indent))),
            vec![
                "╭─────────────┬────────────────────────╮",
                "│ description │ Drafts notes from      │",
                "│             │   merged pull requests │",
                "│ notes       │ one                    │",
                "│             │   two                  │",
                "│ tags        │ a, b                   │",
                "│ author      │ ╭──────┬─────────────╮ │",
                "│             │ │ name │ Aino        │ │",
                "│             │ │      │   Virtanen  │ │",
                "│             │ ╰──────┴─────────────╯ │",
                "╰─────────────┴────────────────────────╯",
            ]
        );
    }

    #[test]
    fn indent_leaves_a_stacked_table_as_it_is() {
        let cells = table(
            &["Name", "Notes"],
            &[&["Helsinki", "Capital of Finland since 1812"]],
        );
        assert_eq!(
            layout(&cells, 20, &with_rows(Rows::Indent)),
            layout(&cells, 20, &styles())
        );
    }

    #[test]
    fn underline_marks_the_last_line_of_each_row_but_the_last() {
        let cells = keyed(vec![
            ("a", cell("one two").into()),
            ("b", cell("three").into()),
        ]);
        let rows = Rows::Underline { color: Some(PAGE) };
        let lines = layout(&cells, 13, &with_rows(rows));
        assert_eq!(
            text(&lines),
            [
                "╭───┬───────╮",
                "│ a │ one   │",
                "│   │ two   │",
                "│ b │ three │",
                "╰───┴───────╯"
            ]
        );
        let underlined = |line: &Line| {
            (line.spans.iter())
                .map(|span| span.style.underline != Underline::None)
                .collect::<Vec<_>>()
        };
        // Only row a's last line, between the outer borders.
        let row = underlined(&lines[2]);
        assert!(!row[0] && !row[row.len() - 1], "{:?}", lines[2]);
        assert!(row[1..row.len() - 1].iter().all(|on| *on), "{:?}", lines[2]);
        assert_eq!(lines[2].spans[1].style.underline_color, Some(PAGE));
        for index in [1, 3] {
            assert!(underlined(&lines[index]).iter().all(|on| !on), "{index}");
        }
    }

    #[test]
    fn a_row_that_ends_in_a_table_is_not_underlined_under_its_border() {
        let inner = keyed(vec![("x", cell("1").into()), ("y", cell("2").into())]);
        let cells = keyed(vec![("a", Cell::Table(inner)), ("b", cell("text").into())]);
        let rows = Rows::Underline { color: Some(PAGE) };
        let lines = layout(&cells, 80, &with_rows(rows));
        assert_eq!(
            text(&lines),
            vec![
                "╭───┬───────────╮",
                "│ a │ ╭───┬───╮ │",
                "│   │ │ x │ 1 │ │",
                "│   │ │ y │ 2 │ │",
                "│   │ ╰───┴───╯ │",
                "│ b │ text      │",
                "╰───┴───────────╯",
            ]
        );
        assert_ends_row_around_its_border(&lines[4]);
        // The inner table still underlines its own row x.
        assert!(
            (lines[2].spans.iter()).any(|span| span.style.underline_color.is_some()),
            "{:?}",
            lines[2]
        );
    }

    /// The inner table's bottom border on `line` closes the row there
    /// itself, so only the rest of the line, the key column, is underlined.
    fn assert_ends_row_around_its_border(line: &Line) {
        let on = |span: &Span| span.style.underline_color.is_some();
        let border = |span: &Span| span.text.contains(['╰', '┴', '╯', '─']);
        assert!(
            (line.spans.iter())
                .filter(|span| border(span))
                .all(|span| !on(span)),
            "{line:?}"
        );
        // The key column's padding, after the outer left border.
        assert!(on(&line.spans[1]), "{line:?}");
    }

    /// The last line of the row before the one keyed `next`.
    fn end_of_row_before<'a>(lines: &'a [Line], next: &str) -> &'a Line {
        let at = (lines.iter())
            .position(|line| style::text(&line.spans).starts_with(&format!("│ {next} ")))
            .expect("the next row");
        &lines[at - 1]
    }

    #[test]
    fn a_box_after_text_in_a_stacked_table_ends_its_row_around_its_border() {
        let sub = keyed(vec![("k", cell("v").into())]);
        let mid = keyed(vec![
            ("longer key", cell("some text").into()),
            ("sub", Cell::Table(sub)),
        ]);
        let cells = keyed(vec![("r", Cell::Table(mid)), ("s", cell("text").into())]);
        let rows = Rows::Underline { color: Some(PAGE) };
        for max in [20, 24] {
            let lines = layout(&cells, max, &with_rows(rows));
            assert_ends_row_around_its_border(end_of_row_before(&lines, "s"));
        }
    }

    #[test]
    fn a_boxed_stack_last_in_a_stacked_stack_ends_its_row_around_its_border() {
        let wide = keyed(vec![(
            "a very long key name",
            cell("value of some length").into(),
        )]);
        let small = keyed(vec![("k", cell("v").into())]);
        let value = Cell::Stack(vec![
            Cell::Table(wide),
            Cell::Stack(vec![Cell::Table(small)]),
        ]);
        let cells = keyed(vec![("r", value), ("s", cell("text").into())]);
        let rows = Rows::Underline { color: Some(PAGE) };
        for max in [20, 24] {
            let lines = layout(&cells, max, &with_rows(rows));
            assert_ends_row_around_its_border(end_of_row_before(&lines, "s"));
        }
    }

    #[test]
    fn a_row_ending_in_a_box_inside_a_stacked_table_ends_around_its_border() {
        let innermost = keyed(vec![("x", cell("1").into())]);
        let inner = keyed(vec![("targets", Cell::Table(innermost))]);
        let cells = keyed(vec![("a", Cell::Table(inner)), ("b", cell("text").into())]);
        let rows = Rows::Underline { color: Some(PAGE) };
        let lines = layout(&cells, 17, &with_rows(rows));
        assert_eq!(
            text(&lines),
            vec![
                "╭───┬───────────╮",
                "│ a │ targets:  │",
                "│   │ ╭───┬───╮ │",
                "│   │ │ x │ 1 │ │",
                "│   │ ╰───┴───╯ │",
                "│ b │ text      │",
                "╰───┴───────────╯",
            ]
        );
        assert_ends_row_around_its_border(&lines[4]);
    }

    #[test]
    fn a_row_ending_in_a_stacked_table_is_underlined_whatever_its_text() {
        let inner = keyed(vec![("x", cell("╰ bar").into())]);
        let cells = keyed(vec![("a", Cell::Table(inner)), ("b", cell("text").into())]);
        let rows = Rows::Underline { color: Some(PAGE) };
        let lines = layout(&cells, 14, &with_rows(rows));
        // Too narrow for a box, the inner table is stacked: its last line
        // is text, even though it starts as a border would.
        let last = lines
            .iter()
            .rposition(|line| style::text(&line.spans).contains("bar"));
        let last = &lines[last.expect("the inner table's text")];
        assert!(
            (last.spans.iter()).any(|span| span.style.underline_color.is_some()),
            "{last:?}"
        );
    }
}
