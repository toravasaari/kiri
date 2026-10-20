//! Tables drawn with box-drawing characters, or stacked when too narrow.

use unicode_width::UnicodeWidthStr;

use super::style::{self, Line, Span, Style, push};
use super::wrap::{keeps_together, width, words, wrap};
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
        }
    }

    /// The cells it would rather keep on one line: text kept together, or
    /// a table as a box.
    fn together(&self) -> usize {
        match self {
            Self::Text(spans) => longest_together(spans),
            Self::Table(cells) => boxed_floor(cells),
            Self::Stack(parts) => parts.iter().map(Cell::together).max().unwrap_or(0),
        }
    }

    /// Its lines in `max` cells.
    fn lines(&self, max: usize, styles: &Styles) -> Vec<Vec<Span>> {
        match self {
            Self::Text(spans) => wrap(spans, max),
            Self::Table(cells) => (layout(cells, max, styles).into_iter())
                .map(|line| line.spans)
                .collect(),
            Self::Stack(parts) => stack_lines(parts, max, styles),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Text(spans) => is_empty(spans),
            Self::Table(cells) => cells.rows.is_empty(),
            Self::Stack(parts) => parts.iter().all(Cell::is_empty),
        }
    }
}

/// The lines of a stack in `max` cells. Its tables are all boxes, or all
/// stacked when one of them does not fit as a box, so that they look
/// alike; stacked, its parts are separated by rules.
fn stack_lines(parts: &[Cell], max: usize, styles: &Styles) -> Vec<Vec<Span>> {
    let boxed = (parts.iter()).all(|part| match part {
        Cell::Table(cells) => boxed_floor(cells) <= max,
        _ => true,
    });
    let mut lines = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        if !boxed && index > 0 {
            lines.push(vec![Span::new(&"─".repeat(max), styles.border.clone())]);
        }
        match part {
            Cell::Table(cells) if !boxed => {
                let stacked = stacked(cells, column_count(cells), max, styles);
                lines.extend(stacked.into_iter().map(|line| line.spans));
            }
            part => lines.extend(part.lines(max, styles)),
        }
    }
    lines
}

pub struct Styles {
    pub border: Style,
    /// For the colon after a header in a stacked table.
    pub header: Style,
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
    let border = &styles.border;
    let columns = column_count(cells);
    if columns == 0 {
        return Vec::new();
    }
    let Some(widths) = column_widths(cells, columns, max) else {
        return stacked(cells, columns, max, styles);
    };

    let mut lines = vec![border_line(&widths, ["╭", "┬", "╮"], border)];
    if !cells.header_column {
        let header: Vec<Cell> = cells.header.iter().cloned().map(Cell::Text).collect();
        lines.extend(row_lines(&header, &widths, &cells.alignments, styles));
        lines.push(border_line(&widths, ["├", "┼", "┤"], border));
    }
    for row in &cells.rows {
        lines.extend(row_lines(row, &widths, &cells.alignments, styles));
    }
    lines.push(border_line(&widths, ["╰", "┴", "╯"], border));
    lines
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
fn stacked(cells: &Cells, columns: usize, max: usize, styles: &Styles) -> Vec<Line> {
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
    for (index, row) in entries.into_iter().enumerate() {
        if index > 0 && !cells.header_column {
            lines.push(Line::new(vec![Span::new(
                &"─".repeat(max),
                styles.border.clone(),
            )]));
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
                lines.extend(part.lines(max, styles).into_iter().map(Line::new));
            }
        }
    }
    lines
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

fn border_line(widths: &[usize], [left, middle, right]: [&str; 3], style: &Style) -> Line {
    let segments: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
    let text = format!("{left}{}{right}", segments.join(middle));
    Line::new(vec![Span::new(&text, style.clone())])
}

fn row_lines(
    row: &[Cell],
    widths: &[usize],
    alignments: &[Alignment],
    styles: &Styles,
) -> Vec<Line> {
    let border = &styles.border;
    let empty = Vec::new();
    let wrapped: Vec<Vec<Vec<Span>>> = widths
        .iter()
        .enumerate()
        .map(|(column, &w)| match row.get(column) {
            Some(cell) => cell.lines(w, styles),
            None => vec![Vec::new()],
        })
        .collect();
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
            Line::new(spans)
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
        }
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
            text(&layout(&cells, 80, &styles())),
            vec![
                "╭──────┬────────────────╮",
                "│ name │ kiri           │",
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
            text(&layout(&cells, 80, &styles())),
            vec![
                "╭────────┬──────────────────────────────────╮",
                "│ title  │ Notes on a long and winding road │",
                "│ author │ ╭──────┬─────────────╮           │",
                "│        │ │ name │ Aino        │           │",
                "│        │ │ site │ example.com │           │",
                "│        │ ╰──────┴─────────────╯           │",
                "╰────────┴──────────────────────────────────╯",
            ]
        );
        // Narrower, the text wraps while the table in the cell keeps its
        // box.
        let lines = text(&layout(&cells, 36, &styles()));
        assert!(lines.iter().all(|line| line.width() <= 36), "{lines:#?}");
        assert_eq!(
            lines,
            vec![
                "╭────────┬─────────────────────────╮",
                "│ title  │ Notes on a long and     │",
                "│        │ winding road            │",
                "│ author │ ╭──────┬─────────────╮  │",
                "│        │ │ name │ Aino        │  │",
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
            text(&layout(&cells, 80, &styles())),
            vec![
                "╭──────────┬───────────────────╮",
                "│ features │ ╭───────┬──────╮  │",
                "│          │ │ title │ Fast │  │",
                "│          │ │ icon  │ ⚡   │  │",
                "│          │ ╰───────┴──────╯  │",
                "│          │ and               │",
                "│          │ ╭───────┬───────╮ │",
                "│          │ │ title │ Small │ │",
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
            text(&layout(&cells, 28, &styles())),
            vec![
                "╭─────────────┬────────────╮",
                "│ description │ How the    │",
                "│             │ build is   │",
                "│             │ set up     │",
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
            cell_text(&stops.lines(18, &styles())),
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
            cell_text(&stops.lines(21, &styles()))[4],
            "╭──────┬────────────╮"
        );
    }
}
