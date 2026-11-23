//! Front matter, as a table of its keys and values or as a code block.

use crate::typeset::grid::{self, Leaves};
use crate::typeset::inputs::FrontMatterStyle;
use crate::typeset::layout::highlight;
use crate::typeset::layout::table::{self, Content};
use crate::typeset::layout::{Layout, code};
use crate::typeset::model::{Block, FrontMatter};
use crate::typeset::style::{Line, Origin, Spaces, Span, Style};
use crate::typeset::text::{self, FrontMatterTexts, Part};

impl Layout<'_> {
    pub(super) fn front_matter(
        &self,
        block: &Block,
        front_matter: &FrontMatter,
        width: usize,
    ) -> Vec<Line> {
        // Code that is the front matter's source is the text of it when the
        // text is the source; shown as code in place of keys and values it
        // shows none of them.
        let code = || {
            self.text_is(text::number(block, Part::Source));
            self.code_block(
                Some(&front_matter.language),
                &front_matter.source,
                false,
                width,
            )
        };
        match (self.front_matter, text::front_matter_texts(front_matter)) {
            (FrontMatterStyle::Hide, _) | (_, FrontMatterTexts::None) => Vec::new(),
            (FrontMatterStyle::Code, _) | (FrontMatterStyle::Table, FrontMatterTexts::Source) => {
                code()
            }
            (FrontMatterStyle::Table, FrontMatterTexts::Leaves(entries)) => {
                let styles = self.table_styles(self.key_style());
                let leaves = Numbered { layout: self };
                table::layout(&grid::front_matter_grid(entries, &leaves), width, &styles)
            }
        }
    }

    fn key_style(&self) -> Style {
        self.style(Style {
            bold: true,
            ..Style::default()
        })
    }
}

/// Whether the texts of `block` are on no line when front matter shows as
/// `style`: front matter that is hidden, or shown as its source in place of
/// its keys and values.
pub(crate) fn texts_hidden(style: FrontMatterStyle, block: &Block) -> bool {
    match block {
        Block::FrontMatter(front_matter) => match style {
            FrontMatterStyle::Hide => true,
            FrontMatterStyle::Code => matches!(
                text::front_matter_texts(front_matter),
                FrontMatterTexts::Leaves(_)
            ),
            FrontMatterStyle::Table => false,
        },
        _ => false,
    }
}

/// The leaves of front matter's table, which are its texts, and the layout
/// that makes them. The grid numbers them.
struct Numbered<'a, 'l> {
    layout: &'a Layout<'l>,
}

impl Numbered<'_, '_> {
    /// What text number `number` shows of its own bytes: all of `text`.
    fn shown(number: usize, text: &str) -> Origin {
        Origin::exact(number as u32, 0..text.len() as u32)
    }
}

impl Leaves<Content> for Numbered<'_, '_> {
    fn key(&self, number: usize, key: &str) -> Content {
        let span = Span::new(key, self.layout.key_style());
        Content::Text(vec![span.showing(Self::shown(number, key))])
    }

    fn text(&self, number: usize, text: &str) -> Content {
        let span = Span::new(
            text,
            Style {
                spaces: Spaces::Collapse,
                ..Style::default()
            },
        );
        Content::Text(vec![span.showing(Self::shown(number, text))])
    }

    fn empty(&self, number: usize) -> Content {
        self.text(number, "")
    }

    /// `yaml` as code in a cell: highlighted on the code background as a
    /// fenced ```yaml block is, or as it is written without colour.
    fn yaml(&self, number: usize, yaml: &str) -> Content {
        let this = self.layout;
        let rows = code::code_rows(yaml, Some(number as u32), 0);
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
