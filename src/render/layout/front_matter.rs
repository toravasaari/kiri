//! Front matter, as a table of its keys and values or as a code block.

use super::{Layout, code};
use crate::model::{FrontMatter, Value};
use crate::render::style::{Line, Spaces, Span, Style};
use crate::render::table::{self, Cell, Cells};
use crate::render::{FrontMatterStyle, highlight};

/// The deepest table: the top table is level 1, a table in one of its
/// cells level 2. Values nested deeper show as YAML-like text.
const LEVELS: usize = 3;

impl Layout<'_> {
    pub(super) fn front_matter(&self, front_matter: &FrontMatter, width: usize) -> Vec<Line> {
        let code = || self.code_block(Some(&front_matter.language), &front_matter.source, width);
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
    fn map_table(&self, entries: &[(String, Value)], level: usize) -> Cells {
        let key_style = self.key_style();
        Cells {
            header: Vec::new(),
            rows: (entries.iter())
                .map(|(key, value)| {
                    let key = Cell::Text(vec![Span::new(key, key_style.clone())]);
                    vec![key, self.value_cell(value, level)]
                })
                .collect(),
            alignments: Vec::new(),
            header_column: true,
        }
    }

    /// `value` in a cell of a table at `level`.
    fn value_cell(&self, value: &Value, level: usize) -> Cell {
        let text = |text: &str, spaces| {
            Cell::Text(vec![Span::new(
                text,
                Style {
                    spaces,
                    ..Style::default()
                },
            )])
        };
        match value {
            Value::Map(entries) if entries.is_empty() => text("", Spaces::Collapse),
            Value::List(items) if items.iter().all(is_scalar) => {
                let items: Vec<&str> = items.iter().filter_map(scalar).collect();
                text(&items.join(", "), Spaces::Collapse)
            }
            Value::Map(entries) if level < LEVELS => {
                Cell::Table(self.map_table(entries, level + 1))
            }
            Value::List(items) if level < LEVELS => Cell::Stack(
                (items.iter())
                    .map(|item| match item {
                        Value::Map(entries) if !entries.is_empty() => {
                            Cell::Table(self.map_table(entries, level + 1))
                        }
                        item => self.value_cell(item, level),
                    })
                    .collect(),
            ),
            Value::Map(_) | Value::List(_) => {
                let mut yaml = String::new();
                write_yaml(value, 0, &mut yaml);
                self.yaml_code(&yaml)
            }
            scalar => text(self::scalar(scalar).unwrap_or_default(), Spaces::Collapse),
        }
    }

    /// `yaml` as code in a cell: highlighted on the code background as a
    /// fenced ```yaml block is, or as it is written without colour.
    fn yaml_code(&self, yaml: &str) -> Cell {
        let highlighted = if self.caps.styling && self.caps.color {
            highlight::highlight(yaml, Some("yaml"), self.code_theme())
        } else {
            None
        };
        let Some(highlighted) = highlighted else {
            return Cell::Code {
                lines: (yaml.lines())
                    .map(|line| vec![Span::new(line, Style::default())])
                    .collect(),
                fill: Style::default(),
            };
        };
        let fill = Style {
            bg: highlighted.background,
            ..Style::default()
        };
        Cell::Code {
            lines: (highlighted.lines.into_iter())
                .map(|line| code::on_background(line, highlighted.background))
                .collect(),
            fill,
        }
    }
}

fn is_empty_collection(value: &Value) -> bool {
    match value {
        Value::Map(entries) => entries.is_empty(),
        Value::List(items) => items.is_empty(),
        _ => false,
    }
}

fn is_scalar(value: &Value) -> bool {
    scalar(value).is_some()
}

/// A value that is not a list or a map as text: strings as written, null
/// as nothing.
fn scalar(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) | Value::Number(text) => Some(text),
        Value::Bool(true) => Some("true"),
        Value::Bool(false) => Some("false"),
        Value::Null => Some(""),
        Value::List(_) | Value::Map(_) => None,
    }
}

/// Writes a list or a map as YAML in block style, indented by `indent`
/// spaces: kiri's own rendering of its tree, whatever format the front
/// matter was written in.
fn write_yaml(value: &Value, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match value {
        Value::Map(entries) => {
            for (key, value) in entries {
                out.push_str(&format!("{pad}{key}:"));
                write_nested(value, indent, out);
            }
        }
        Value::List(items) => {
            for item in items {
                match item {
                    // A list or a map starts after the item's dash.
                    Value::Map(_) | Value::List(_) if !is_empty_collection(item) => {
                        write_item(item, indent, out);
                    }
                    item => {
                        out.push_str(&format!("{pad}-"));
                        write_nested(item, indent, out);
                    }
                }
            }
        }
        scalar => write_scalar(scalar, indent, out),
    }
}

/// Writes a list or a map that is an item of a list at `indent`, its
/// first line after the item's dash.
fn write_item(item: &Value, indent: usize, out: &mut String) {
    let mut nested = String::new();
    write_yaml(item, indent + 2, &mut nested);
    out.push_str(&format!(
        "{}- {}",
        " ".repeat(indent),
        &nested[indent + 2..]
    ));
}

/// Writes `value` after a key or a dash at `indent`: a scalar on the same
/// line, a list or a map indented under it. An empty list or map shows as
/// `[]` or `{}`.
fn write_nested(value: &Value, indent: usize, out: &mut String) {
    match value {
        Value::Map(entries) if entries.is_empty() => out.push_str(" {}\n"),
        Value::List(items) if items.is_empty() => out.push_str(" []\n"),
        Value::Map(_) | Value::List(_) => {
            out.push('\n');
            write_yaml(value, indent + 2, out);
        }
        scalar => write_scalar(scalar, indent + 2, out),
    }
}

/// Writes a scalar after `: ` or `- `, ending its line; null leaves the
/// line empty. A string of several lines is a literal block indented by
/// `indent`.
fn write_scalar(value: &Value, indent: usize, out: &mut String) {
    let text = scalar(value).unwrap_or_default();
    if text.trim_end_matches('\n').contains('\n') {
        out.push_str(" |\n");
        for line in text.lines() {
            out.push_str(&format!("{}{line}\n", " ".repeat(indent)));
        }
    } else if text.is_empty() {
        out.push('\n');
    } else {
        out.push_str(&format!(" {}\n", text.trim_end_matches('\n')));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(text: &str) -> Value {
        Value::String(text.into())
    }

    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Map(
            (entries.iter())
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    fn yaml(value: &Value) -> String {
        let mut out = String::new();
        write_yaml(value, 0, &mut out);
        out
    }

    #[test]
    fn writes_maps_and_lists_in_block_style() {
        let value = map(&[
            ("opt", Value::Number("3".into())),
            ("lto", Value::Bool(true)),
            ("env", Value::List(vec![string("CI=1"), string("LANG=C")])),
            ("owner", Value::Null),
            (
                "stops",
                Value::List(vec![
                    map(&[
                        ("name", string("Harbour")),
                        ("km", Value::Number("0".into())),
                    ]),
                    Value::List(vec![string("a")]),
                ]),
            ),
            ("none", Value::List(Vec::new())),
            ("empty", map(&[])),
        ]);
        assert_eq!(
            yaml(&value),
            "opt: 3\n\
             lto: true\n\
             env:\n  - CI=1\n  - LANG=C\n\
             owner:\n\
             stops:\n  - name: Harbour\n    km: 0\n  - - a\n\
             none: []\n\
             empty: {}\n"
        );
    }

    #[test]
    fn a_string_of_several_lines_is_a_literal_block() {
        let value = map(&[(
            "notes",
            map(&[
                ("text", string("one\ntwo\n")),
                ("folded", string("one two\n")),
            ]),
        )]);
        assert_eq!(
            yaml(&value),
            "notes:\n  text: |\n    one\n    two\n  folded: one two\n"
        );
    }
}
