//! Tables drawn with box-drawing characters, or stacked when too narrow.

use unicode_width::UnicodeWidthStr;

use super::style::{Line, Span, Style, push};
use super::wrap::{keeps_together, width, words, wrap};
use crate::model::Alignment;

/// Cells as styled spans; rows may be shorter than the header.
pub struct Cells {
    pub header: Vec<Vec<Span>>,
    pub rows: Vec<Vec<Vec<Span>>>,
    pub alignments: Vec<Alignment>,
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
    let columns = cells
        .rows
        .iter()
        .map(Vec::len)
        .chain([cells.header.len(), cells.alignments.len()])
        .max()
        .unwrap_or(0);
    if columns == 0 {
        return Vec::new();
    }
    let Some(widths) = column_widths(cells, columns, max) else {
        return stacked(cells, columns, max, styles);
    };

    let mut lines = vec![border_line(&widths, ["╭", "┬", "╮"], border)];
    lines.extend(row_lines(&cells.header, &widths, &cells.alignments, border));
    lines.push(border_line(&widths, ["├", "┼", "┤"], border));
    for row in &cells.rows {
        lines.extend(row_lines(row, &widths, &cells.alignments, border));
    }
    lines.push(border_line(&widths, ["╰", "┴", "╯"], border));
    lines
}

/// Widths that fit in `max` without splitting short words, or `None`.
fn column_widths(cells: &Cells, columns: usize, max: usize) -> Option<Vec<usize>> {
    let mut widths = vec![1; columns];
    // No column is narrower than its longest short word. That covers its
    // widest character too, two cells for CJK and emoji, which would push
    // its borders out of line.
    let mut minimums = vec![1; columns];
    let mut together = vec![0; columns];
    for row in std::iter::once(&cells.header).chain(&cells.rows) {
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(width(cell));
            minimums[column] = minimums[column].max(longest_word(cell).min(SHORT_WORD));
            together[column] = together[column].max(longest_together(cell));
        }
    }
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
/// value goes on the lines under its header.
fn stacked(cells: &Cells, columns: usize, max: usize, styles: &Styles) -> Vec<Line> {
    let empty = Vec::new();
    let labels: Vec<Vec<Span>> = (0..columns)
        .map(|column| {
            let header = cells.header.get(column).unwrap_or(&empty);
            let mut label = header.clone();
            if !is_empty(header) {
                push(&mut label, ":", &styles.header);
            }
            label
        })
        .collect();
    let longest = labels.iter().map(|label| width(label)).max().unwrap_or(0);
    let tight = max < longest + 1 + SHORT_WORD;
    // A table with a header only still shows its headers.
    let rows: &[Vec<Vec<Span>>] = if cells.rows.is_empty() {
        &[Vec::new()]
    } else {
        &cells.rows
    };

    let mut lines = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            lines.push(Line::new(vec![Span::new(
                &"─".repeat(max),
                styles.border.clone(),
            )]));
        }
        for (label, value) in labels
            .iter()
            .zip(row.iter().chain(std::iter::repeat(&empty)))
        {
            let mut parts: Vec<Vec<Span>> = Vec::new();
            match (is_empty(label), is_empty(value)) {
                (true, true) => {}
                (true, false) => parts.push(value.clone()),
                (false, true) => parts.push(label.clone()),
                (false, false) if tight => parts.extend([label.clone(), value.clone()]),
                (false, false) => {
                    let mut spans = label.clone();
                    push(&mut spans, " ", &Style::default());
                    spans.extend(value.iter().cloned());
                    parts.push(spans);
                }
            }
            for part in parts {
                lines.extend(wrap(&part, max).into_iter().map(Line::new));
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
    let text: String = cell.iter().map(|span| span.text.as_str()).collect();
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
    row: &[Vec<Span>],
    widths: &[usize],
    alignments: &[Alignment],
    border: &Style,
) -> Vec<Line> {
    let empty = Vec::new();
    let wrapped: Vec<Vec<Vec<Span>>> = widths
        .iter()
        .enumerate()
        .map(|(column, &w)| wrap(row.get(column).unwrap_or(&empty), w))
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
        lines
            .iter()
            .map(|line| line.spans.iter().map(|span| span.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn aligns_cells_within_columns() {
        let cells = Cells {
            header: vec![cell("left"), cell("right")],
            rows: vec![vec![cell("a"), cell("b")]],
            alignments: vec![Alignment::Left, Alignment::Right],
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
            rows: vec![vec![cell("東京"), cell("🦀")]],
            alignments: vec![],
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
            rows: vec![vec![cell("1")]],
            alignments: vec![],
        };
        assert_eq!(text(&layout(&cells, 80, &styles()))[3], "│ 1 │   │");
    }

    fn table(header: &[&str], rows: &[&[&str]]) -> Cells {
        Cells {
            header: header.iter().map(|text| cell(text)).collect(),
            rows: (rows.iter())
                .map(|row| row.iter().map(|text| cell(text)).collect())
                .collect(),
            alignments: vec![],
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
                cell("a fairly long note about the step"),
                code("git commit -m message", spaces),
            ]],
            alignments: vec![],
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
                cell("https://example.com/a/very/long/path"),
                code("git commit", Spaces::KeepTogether),
            ]],
            alignments: vec![],
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
                vec![cell("Helsinki"), vec![Span::new("capital", code.clone())]],
                vec![cell("Tampere"), cell("inland")],
            ],
            alignments: vec![],
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
}
