//! Front matter, as a table of its keys and values or as a code block.

use crate::render::FrontMatterStyle;
use crate::typeset::grid::{Cell, Grid};
use crate::typeset::layout::highlight;
use crate::typeset::layout::table::{self, Content};
use crate::typeset::layout::{Layout, code};
use crate::typeset::model::{FrontMatter, Value};
use crate::typeset::style::{Line, Spaces, Span, Style};

impl Layout<'_> {
    pub(super) fn front_matter(&self, front_matter: &FrontMatter, width: usize) -> Vec<Line> {
        let code = || {
            self.code_block(
                Some(&front_matter.language),
                &front_matter.source,
                false,
                width,
            )
        };
        match self.front_matter {
            FrontMatterStyle::Hide => Vec::new(),
            FrontMatterStyle::Code => code(),
            FrontMatterStyle::Table => match &front_matter.value {
                _ if front_matter.source.trim().is_empty() => Vec::new(),
                Some(Ok(Value::Map(entries))) => {
                    let styles = self.table_styles(self.key_style());
                    table::layout(&self.map_table(entries, 1), width, &styles)
                }
                // Another language, content that does not parse, or
                // content that is not a map.
                _ => code(),
            },
        }
    }

    fn key_style(&self) -> Style {
        self.style(Style {
            bold: true,
            ..Style::default()
        })
    }

    /// `entries` as a table at `level`, its keys in a header column.
    fn map_table(&self, entries: &[(String, Value)], level: usize) -> Grid<Content> {
        let key_style = self.key_style();
        Grid {
            rows: (entries.iter())
                .map(|(key, value)| {
                    let key = vec![Span::new(key, key_style.clone())].into();
                    vec![key, self.value_cell(value, level)]
                })
                .collect(),
            alignments: Vec::new(),
            header_row: false,
            header_column: true,
        }
    }

    /// `value` in a cell of a table at `level`.
    fn value_cell(&self, value: &Value, level: usize) -> Cell<Content> {
        let text = |text: &str, spaces| {
            Cell::from(vec![Span::new(
                text,
                Style {
                    spaces,
                    ..Style::default()
                },
            )])
        };
        match value {
            Value::Map(entries) if entries.is_empty() => text("", Spaces::Collapse),
            Value::List(items) if items.iter().all(Value::is_scalar) => {
                let items: Vec<&str> = items.iter().filter_map(Value::scalar).collect();
                text(&items.join(", "), Spaces::Collapse)
            }
            Value::Map(entries) if level < Value::TABLE_LEVELS => {
                Cell::Table(self.map_table(entries, level + 1))
            }
            Value::List(items) if level < Value::TABLE_LEVELS => Cell::Stack(
                (items.iter())
                    .map(|item| match item {
                        Value::Map(entries) if !entries.is_empty() => {
                            Cell::Table(self.map_table(entries, level + 1))
                        }
                        item => self.value_cell(item, level),
                    })
                    .collect(),
            ),
            Value::Map(_) | Value::List(_) => self.yaml_code(&value.yaml()),
            scalar => text(scalar.scalar().unwrap_or_default(), Spaces::Collapse),
        }
    }

    /// `yaml` as code in a cell: highlighted on the code background as a
    /// fenced ```yaml block is, or as it is written without colour.
    fn yaml_code(&self, yaml: &str) -> Cell<Content> {
        let highlighted = if self.caps.styling && self.caps.color {
            highlight::highlight(yaml, Some("yaml"), self.code_theme())
        } else {
            None
        };
        let Some(highlighted) = highlighted else {
            return Cell::Leaf(Content::Code {
                lines: (yaml.lines())
                    .map(|line| vec![Span::new(line, Style::default())])
                    .collect(),
                fill: Style::default(),
            });
        };
        let fill = Style {
            bg: highlighted.background,
            ..Style::default()
        };
        Cell::Leaf(Content::Code {
            lines: (highlighted.lines.into_iter())
                .map(|line| code::on_background(line, highlighted.background))
                .collect(),
            fill,
        })
    }
}
