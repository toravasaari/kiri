//! Tables drawn with box-drawing characters.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::style::{Line, Span, Style};
use super::wrap::{width, wrap};
use crate::model::Alignment;

/// Cells as styled spans; rows may be shorter than the header.
pub struct Cells {
    pub header: Vec<Vec<Span>>,
    pub rows: Vec<Vec<Vec<Span>>>,
    pub alignments: Vec<Alignment>,
}

/// Columns shrink, widest first, until the table fits in `max` cells; cell
/// text then wraps inside its column.
pub fn layout(cells: &Cells, max: usize, border: &Style) -> Vec<Line> {
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
    let widths = column_widths(cells, columns, max);

    let mut lines = vec![border_line(&widths, ["╭", "┬", "╮"], border)];
    lines.extend(row_lines(&cells.header, &widths, &cells.alignments, border));
    lines.push(border_line(&widths, ["├", "┼", "┤"], border));
    for row in &cells.rows {
        lines.extend(row_lines(row, &widths, &cells.alignments, border));
    }
    lines.push(border_line(&widths, ["╰", "┴", "╯"], border));
    lines
}

fn column_widths(cells: &Cells, columns: usize, max: usize) -> Vec<usize> {
    let mut widths = vec![1; columns];
    // A column narrower than its widest character, two cells for CJK and
    // emoji, would push its borders out of line. When the minimums do not
    // fit, the table is wider than `max` instead.
    let mut minimums = vec![1; columns];
    for row in std::iter::once(&cells.header).chain(&cells.rows) {
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(width(cell));
            minimums[column] = minimums[column].max(widest_grapheme(cell));
        }
    }
    // Each column costs its padding and one border, plus the closing border.
    let available = max.saturating_sub(3 * columns + 1).max(columns);
    while widths.iter().sum::<usize>() > available {
        let Some(widest) = (0..columns)
            .filter(|&column| widths[column] > minimums[column])
            .max_by_key(|&column| widths[column])
        else {
            break;
        };
        widths[widest] -= 1;
    }
    widths
}

fn widest_grapheme(cell: &[Span]) -> usize {
    (cell.iter())
        .flat_map(|span| span.text.graphemes(true))
        .map(UnicodeWidthStr::width)
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

    fn cell(text: &str) -> Vec<Span> {
        vec![Span::new(text, Style::default())]
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
            text(&layout(&cells, 80, &Style::default())),
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
    fn shrinks_the_widest_column_and_wraps_its_text() {
        let cells = Cells {
            header: vec![cell("id"), cell("description")],
            rows: vec![vec![cell("1"), cell("a long piece of text")]],
            alignments: vec![],
        };
        let lines = text(&layout(&cells, 20, &Style::default()));
        assert!(lines.iter().all(|line| line.width() <= 20), "{lines:#?}");
        assert_eq!(lines[3], "│ 1  │ a long      │");
        assert_eq!(lines[4], "│    │ piece of    │");
    }

    #[test]
    fn a_column_is_never_narrower_than_its_widest_character() {
        let cells = Cells {
            header: vec![cell("名前"), cell("Emoji")],
            rows: vec![
                vec![cell("東京"), cell("🦀")],
                vec![cell("Oslo"), cell("x")],
            ],
            alignments: vec![],
        };
        let lines = text(&layout(&cells, 8, &Style::default()));
        let widths: Vec<usize> = lines.iter().map(|line| line.width()).collect();
        assert!(widths.iter().all(|&w| w == widths[0]), "{lines:#?}");
        assert!(lines.contains(&"│ 東 │ 🦀 │".to_owned()), "{lines:#?}");
        // A column of narrow characters still shrinks to one cell.
        let cells = Cells {
            header: vec![cell("名前"), cell("ab")],
            rows: vec![],
            alignments: vec![],
        };
        let lines = text(&layout(&cells, 8, &Style::default()));
        assert_eq!(lines[1], "│ 名 │ a │");
    }

    #[test]
    fn fills_missing_cells() {
        let cells = Cells {
            header: vec![cell("a"), cell("b")],
            rows: vec![vec![cell("1")]],
            alignments: vec![],
        };
        assert_eq!(text(&layout(&cells, 80, &Style::default()))[3], "│ 1 │   │");
    }
}
