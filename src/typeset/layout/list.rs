//! Bullet, numbered and task lists.

use unicode_width::UnicodeWidthStr;

use crate::typeset::layout::{Context, Layout};
use crate::typeset::model::List;
use crate::typeset::style::{Line, Span, Style};

impl Layout<'_> {
    pub(super) fn list(&self, list: &List, width: usize, context: Context) -> Vec<Line> {
        // A styled bullet list shows a task's checkbox in place of the bullet.
        let boxes_as_markers = list.start.is_none() && self.caps.styling;
        let markers: Vec<String> = (list.items.iter().enumerate())
            .map(|(index, item)| match (list.start, item.checked) {
                (Some(start), _) => format!("{}.", start + index as u64),
                (None, Some(checked)) if boxes_as_markers => {
                    checkbox(checked, true).trim_end().into()
                }
                (None, _) if self.caps.styling => {
                    self.look.cells.bullets.at(context.list_depth).into()
                }
                (None, _) => "-".into(),
            })
            .collect();
        let marker_width = markers.iter().map(|m| m.width()).max().unwrap_or(0) + 1;
        let marker_style = self.style(Style::fg(self.look.list_marker));
        let inner = Context {
            nested: true,
            list_depth: context.list_depth + 1,
            tight: list.tight,
        };

        let mut lines = Vec::new();
        for (index, (item, marker)) in list.items.iter().zip(&markers).enumerate() {
            if index > 0 && !list.tight {
                lines.push(Line::blank());
            }
            let mut first = vec![Span::new(
                &format!("{marker:>width$} ", width = marker_width - 1),
                marker_style.clone(),
            )];
            let mut item_width = width.saturating_sub(marker_width).max(1);
            if let Some(checked) = item.checked
                && !boxes_as_markers
            {
                let box_text = checkbox(checked, self.caps.styling);
                first.push(Span::new(box_text, marker_style.clone()));
                item_width = item_width.saturating_sub(box_text.width()).max(1);
            }
            let prefix_width = width - item_width;
            let mut item_lines = self.blocks(&item.blocks, item_width, inner);
            if item_lines.is_empty() {
                item_lines.push(Line::blank());
            }
            // The marker is on the text row of a split first line.
            let marked = item_lines
                .first()
                .map_or(0, |line| usize::from(line.marker_row));
            for (line_index, line) in item_lines.iter_mut().enumerate() {
                if line_index == marked {
                    let mut first = first.clone();
                    // An empty item ends at its marker, without the space.
                    if line.is_blank()
                        && let Some(last) = first.last_mut()
                    {
                        last.text.truncate(last.text.trim_end().len());
                    }
                    for span in first.into_iter().rev() {
                        line.marker(span);
                    }
                } else if !line.is_blank() {
                    line.indent(Span::new(&" ".repeat(prefix_width), Style::default()));
                }
            }
            lines.extend(item_lines);
        }
        lines
    }
}

fn checkbox(checked: bool, styled: bool) -> &'static str {
    match (checked, styled) {
        (true, true) => "☑ ",
        (false, true) => "☐ ",
        (true, false) => "[x] ",
        (false, false) => "[ ] ",
    }
}
