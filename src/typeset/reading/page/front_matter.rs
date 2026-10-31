//! Front matter: its keys and values as a grid, with the source in a code block
//! when it is shown as source.

use crate::typeset::model::{FrontMatter, Inline, Value};
use crate::typeset::reading::page::{Cell, Flow, FrontMatterShown, Grid, LINE_HEIGHT};

impl Flow<'_> {
    /// Lays front matter out at `x` in `width` pixels: a table of its keys
    /// and values when it has keys, otherwise, or when it is asked for as
    /// code, a code block; an empty one is nothing. It does not use the
    /// area: its cells wrap and it never scrolls.
    pub(super) fn front_matter(&mut self, front_matter: &FrontMatter, x: f32, width: f32) {
        if front_matter.source.trim().is_empty() {
            return;
        }
        // A blank line above it, so that it does not touch the page's top.
        self.y += LINE_HEIGHT * self.look.size;
        match (&front_matter.value, self.front_matter) {
            (Some(Ok(Value::Map(entries))), FrontMatterShown::Table) => {
                self.nested(&map_grid(entries, 1), x, width, true);
            }
            _ => self.code(Some(&front_matter.language), &front_matter.source, x, width),
        }
    }
}

/// `entries` as a table at `level`, its keys in a column of their own.
pub(super) fn map_grid(entries: &[(String, Value)], level: usize) -> Grid {
    Grid {
        rows: (entries.iter())
            .map(|(key, value)| {
                vec![
                    Cell::Text(vec![Inline::Text(key.clone())]),
                    value_cell(value, level),
                ]
            })
            .collect(),
        alignments: Vec::new(),
        header_row: false,
        header_column: true,
    }
}

/// `value` in a cell of a table at `level`.
fn value_cell(value: &Value, level: usize) -> Cell {
    let text = |text: &str| Cell::Text(vec![Inline::Text(text.to_owned())]);
    match value {
        Value::Map(entries) if entries.is_empty() => Cell::Text(Vec::new()),
        Value::List(items) if items.iter().all(Value::is_scalar) => {
            let items: Vec<&str> = items.iter().filter_map(Value::scalar).collect();
            text(&items.join(", "))
        }
        Value::Map(entries) if level < Value::TABLE_LEVELS => {
            Cell::Table(map_grid(entries, level + 1))
        }
        Value::List(items) if level < Value::TABLE_LEVELS => Cell::Stack(
            (items.iter())
                .map(|item| match item {
                    Value::Map(entries) if !entries.is_empty() => {
                        Cell::Table(map_grid(entries, level + 1))
                    }
                    item => value_cell(item, level),
                })
                .collect(),
        ),
        // Nested deeper than tables go, as code.
        Value::Map(_) | Value::List(_) => {
            let yaml = value.yaml();
            let mut lines = Vec::new();
            for (number, line) in yaml.lines().enumerate() {
                if number > 0 {
                    lines.push(Inline::LineBreak);
                }
                lines.push(Inline::Code(line.to_owned()));
            }
            Cell::Text(lines)
        }
        scalar => text(scalar.scalar().unwrap_or_default()),
    }
}
