//! Front matter, as a table of its keys and values or as a code block.

use std::cell::Cell;

use crate::render::FrontMatterStyle;
use crate::typeset::grid::{self, Leaves};
use crate::typeset::layout::highlight;
use crate::typeset::layout::table::{self, Content};
use crate::typeset::layout::{Layout, code};
use crate::typeset::model::{FrontMatter, Value};
use crate::typeset::style::{Line, Origin, Spaces, Span, Style};

impl Layout<'_> {
    pub(super) fn front_matter(&self, front_matter: &FrontMatter, width: usize) -> Vec<Line> {
        // Code that is the front matter's source is the text of it when the
        // text is the source; shown as code in place of keys and values it
        // shows none of them.
        let code = |source_is_text: bool| {
            self.text_is(source_is_text.then_some(0));
            self.code_block(
                Some(&front_matter.language),
                &front_matter.source,
                false,
                width,
            )
        };
        let keys_and_values = matches!(&front_matter.value, Some(Ok(Value::Map(_))))
            && !front_matter.source.trim().is_empty();
        match self.front_matter {
            FrontMatterStyle::Hide => Vec::new(),
            FrontMatterStyle::Code => code(!keys_and_values),
            FrontMatterStyle::Table => match &front_matter.value {
                _ if front_matter.source.trim().is_empty() => Vec::new(),
                Some(Ok(Value::Map(entries))) => {
                    let styles = self.table_styles(self.key_style());
                    let leaves = Numbered {
                        layout: self,
                        next: Cell::new(0),
                    };
                    table::layout(&grid::front_matter_grid(entries, &leaves), width, &styles)
                }
                // Another language, content that does not parse, or
                // content that is not a map.
                _ => code(true),
            },
        }
    }

    fn key_style(&self) -> Style {
        self.style(Style {
            bold: true,
            ..Style::default()
        })
    }
}

/// The leaves of front matter's table, which are its texts in the order they
/// are made in, and the layout that makes them.
struct Numbered<'a, 'l> {
    layout: &'a Layout<'l>,
    next: Cell<u32>,
}

impl Numbered<'_, '_> {
    /// The number of the next text, and what it shows of its own bytes: all
    /// of `text`.
    fn shown(&self, text: &str) -> Origin {
        let number = self.next.replace(self.next.get() + 1);
        Origin::exact(number, 0..text.len() as u32)
    }
}

impl Leaves<Content> for Numbered<'_, '_> {
    fn key(&self, key: &str) -> Content {
        let span = Span::new(key, self.layout.key_style());
        Content::Text(vec![span.showing(self.shown(key))])
    }

    fn text(&self, text: &str) -> Content {
        let span = Span::new(
            text,
            Style {
                spaces: Spaces::Collapse,
                ..Style::default()
            },
        );
        Content::Text(vec![span.showing(self.shown(text))])
    }

    fn empty(&self) -> Content {
        self.text("")
    }

    /// `yaml` as code in a cell: highlighted on the code background as a
    /// fenced ```yaml block is, or as it is written without colour.
    fn yaml(&self, yaml: &str) -> Content {
        let this = self.layout;
        let number = self.next.replace(self.next.get() + 1);
        let rows = code::code_rows(yaml, Some(number));
        let highlighted = if this.caps.styling && this.caps.color {
            highlight::highlight(yaml, Some("yaml"), this.code_theme())
        } else {
            None
        };
        let Some(highlighted) = highlighted else {
            return Content::Code {
                lines: rows
                    .iter()
                    .map(|row| vec![row.span(Style::default())])
                    .collect(),
                fill: Style::default(),
            };
        };
        let fill = Style {
            bg: highlighted.background,
            ..Style::default()
        };
        Content::Code {
            lines: (highlighted.lines.into_iter().zip(&rows))
                .map(|(line, row)| code::on_background(row.give(line), highlighted.background))
                .collect(),
            fill,
        }
    }
}
