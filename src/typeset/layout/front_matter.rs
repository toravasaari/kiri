//! Front matter, as a table of its keys and values or as a code block.

use crate::render::FrontMatterStyle;
use crate::typeset::grid::{self, Leaves};
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
                    table::layout(&grid::front_matter_grid(entries, 1, self), width, &styles)
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

    /// `yaml` as code in a cell: highlighted on the code background as a
    /// fenced ```yaml block is, or as it is written without colour.
    fn yaml_code(&self, yaml: &str) -> Content {
        let highlighted = if self.caps.styling && self.caps.color {
            highlight::highlight(yaml, Some("yaml"), self.code_theme())
        } else {
            None
        };
        let Some(highlighted) = highlighted else {
            return Content::Code {
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
        Content::Code {
            lines: (highlighted.lines.into_iter())
                .map(|line| code::on_background(line, highlighted.background))
                .collect(),
            fill,
        }
    }
}

impl Leaves<Content> for Layout<'_> {
    fn key(&self, key: &str) -> Content {
        Content::Text(vec![Span::new(key, self.key_style())])
    }

    fn text(&self, text: &str) -> Content {
        Content::Text(vec![Span::new(
            text,
            Style {
                spaces: Spaces::Collapse,
                ..Style::default()
            },
        )])
    }

    fn empty(&self) -> Content {
        self.text("")
    }

    fn yaml(&self, yaml: &str) -> Content {
        self.yaml_code(yaml)
    }
}
