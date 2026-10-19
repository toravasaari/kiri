//! Document model to styled lines of a fixed width.

mod code;
mod list;
mod quote;

use std::rc::Rc;

use super::Inputs;
use super::image::{self, Placement};
use super::style::{Line, Spaces, Span, Style, push};
use super::table::{self, Cells};
use super::wrap::wrap;
use crate::caps::Capabilities;
use crate::color::Underline;
use crate::images::Images;
use crate::model::{Block, Document, Image, Inline, Table};
use crate::theme::Theme;

pub fn layout(document: &Document, inputs: &Inputs) -> Vec<Line> {
    let Inputs {
        images,
        caps,
        theme,
    } = inputs;
    // Without colours, the layout leaves out what only shows a colour, such
    // as heading fills and the padding around inline code.
    let colorless;
    let theme = if caps.color {
        theme
    } else {
        colorless = theme.clone().without_colors();
        &colorless
    };
    let mut width = usize::from(caps.width);
    if theme.max_width > 0 {
        width = width.min(usize::from(theme.max_width));
    }
    let width = width.max(10);
    let layout = Layout {
        images,
        caps,
        theme,
    };

    // Everything but headings and thematic breaks keeps text_margin free
    // on both sides.
    let margin = usize::from(theme.text_margin).min(width / 4);
    let mut lines = Vec::new();
    layout.top_level(&document.blocks, width, margin, &mut lines);
    lines
}

/// `lines` with `margin` cells before each one that is not blank.
fn with_margin(mut lines: Vec<Line>, margin: usize) -> Vec<Line> {
    if margin > 0 {
        for line in &mut lines {
            if !line.is_blank() {
                line.lead
                    .insert(0, Span::new(&" ".repeat(margin), Style::default()));
            }
        }
    }
    lines
}

/// Appends a block's lines, after a blank line if `blank` and something
/// came before. A block without lines, such as an empty code block, gets
/// no separator either.
fn append_block(lines: &mut Vec<Line>, block_lines: Vec<Line>, blank: bool) {
    if block_lines.is_empty() {
        return;
    }
    if blank && !lines.is_empty() {
        lines.push(Line::blank());
    }
    lines.extend(block_lines);
}

#[derive(Debug, Clone, Copy, Default)]
struct Context {
    /// Inside a quote or list, where scaled or indented headings would break
    /// the prefix.
    nested: bool,
    list_depth: usize,
    /// Tight list item: no blank lines between its blocks.
    tight: bool,
}

struct Layout<'a> {
    images: &'a Images,
    caps: &'a Capabilities,
    theme: &'a Theme,
}

impl Layout<'_> {
    /// Lays out the document's own blocks: headings stay at the edge and
    /// thematic breaks span the terminal; everything else keeps `margin`
    /// free on both sides. The blocks inside a details block are laid out
    /// as the blocks around it are.
    fn top_level(&self, blocks: &[Block], width: usize, margin: usize, lines: &mut Vec<Line>) {
        for block in blocks {
            let block_lines = match block {
                Block::ThematicBreak => vec![self.rule(usize::from(self.caps.width).max(width))],
                Block::Heading { .. } => self.block(block, width, Context::default()),
                Block::Details { summary, blocks } => {
                    let summary = self.summary(summary, width - 2 * margin);
                    append_block(lines, with_margin(summary, margin), true);
                    self.top_level(blocks, width, margin, lines);
                    continue;
                }
                _ => with_margin(
                    self.block(block, width - 2 * margin, Context::default()),
                    margin,
                ),
            };
            append_block(lines, block_lines, true);
        }
    }

    fn blocks(&self, blocks: &[Block], width: usize, context: Context) -> Vec<Line> {
        let mut lines = Vec::new();
        for block in blocks {
            append_block(
                &mut lines,
                self.block(block, width, context),
                !context.tight,
            );
        }
        lines
    }

    fn block(&self, block: &Block, width: usize, context: Context) -> Vec<Line> {
        match block {
            Block::Heading { level, content } => self.heading(*level, content, width, context),
            Block::Paragraph(content) => self.paragraph(content, &Style::default(), width),
            Block::CodeBlock { language, code } => {
                self.code_block(language.as_deref(), code, width)
            }
            Block::BlockQuote(blocks) => self.quote(blocks, width, context),
            Block::Alert {
                kind,
                title,
                blocks,
            } => self.alert(*kind, title.as_deref(), blocks, width, context),
            Block::List(list) => self.list(list, width, context),
            Block::Details { summary, blocks } => {
                let mut lines = self.summary(summary, width);
                append_block(
                    &mut lines,
                    self.blocks(blocks, width, context),
                    !context.tight,
                );
                lines
            }
            Block::Table(table) => self.table(table, width),
            Block::ThematicBreak => vec![self.rule(width)],
            Block::Html(html) => {
                let style = self.muted(Style::default());
                html.lines()
                    .map(|line| Line::new(vec![Span::new(line, style.clone())]))
                    .collect()
            }
            Block::Note(note) => {
                let content = [Inline::Text(note.clone())];
                self.paragraph(&content, &self.muted(Style::default()), width)
            }
        }
    }

    fn heading(&self, level: u8, content: &[Inline], width: usize, context: Context) -> Vec<Line> {
        let heading = self.theme.heading(level);
        let scale = if self.caps.text_sizing && !context.nested {
            heading.scale.max(1)
        } else {
            1
        };
        let style = self.style(Style {
            bold: heading.bold,
            italic: heading.italic,
            fg: heading.color,
            bg: heading.background,
            underline: heading.underline,
            ..Style::default()
        });
        // Indent and fill carry the background but not the underline.
        let background = self.style(Style {
            bg: heading.background,
            ..Style::default()
        });
        let mut text = Vec::new();
        self.inlines(content, &style, &mut text);
        let empty = text.is_empty();
        let mut spans = Vec::new();
        if !self.caps.styling {
            push(
                &mut spans,
                &format!("{} ", "#".repeat(usize::from(level))),
                &style,
            );
        }
        spans.extend(text);
        // A nested heading's indent would land before the quote bar or the
        // list marker, which are prepended later.
        let indent = if context.nested {
            0
        } else {
            usize::from(heading.indent).min(width / 4)
        };
        let lead = if indent > 0 {
            vec![Span::new(&" ".repeat(indent), background.clone())]
        } else {
            Vec::new()
        };
        // In characters of the heading's size.
        let available = (width - indent) / usize::from(scale);
        // An empty heading takes no room, but keeps its rule.
        let mut lines: Vec<Line> = if empty {
            Vec::new()
        } else {
            wrap(&spans, available)
                .into_iter()
                .map(|mut spans| {
                    let mut trail = Vec::new();
                    if background.bg.is_some() {
                        let used = super::wrap::width(&spans);
                        // A wide character can overflow a very narrow heading.
                        let fill = available.saturating_sub(used);
                        push(&mut spans, &" ".repeat(fill), &background);
                        // Scaled text covers a multiple of the scale; cells of
                        // normal size fill the rest of the width.
                        let scaled = (used + fill) * usize::from(scale);
                        let rest = (width - indent).saturating_sub(scaled);
                        push(&mut trail, &" ".repeat(rest), &background);
                    }
                    Line {
                        lead: lead.clone(),
                        spans,
                        image: None,
                        trail,
                        scale,
                    }
                })
                .collect()
        };
        if heading.rule && self.caps.styling {
            // A top-level heading's rule spans the whole terminal, past
            // max_width, to mark the section boundary.
            let rule_width = if context.nested {
                width
            } else {
                usize::from(self.caps.width).max(width)
            };
            lines.push(self.rule(rule_width));
        }
        lines
    }

    /// The summary of a details block, in bold after an open triangle, as
    /// a browser shows an open `<details>`.
    fn summary(&self, summary: &[Inline], width: usize) -> Vec<Line> {
        let style = self.style(Style {
            bold: true,
            ..Style::default()
        });
        let mut spans = Vec::new();
        push(&mut spans, "▼ ", &style);
        self.inlines(summary, &style, &mut spans);
        wrap(&spans, width).into_iter().map(Line::new).collect()
    }

    fn paragraph(&self, content: &[Inline], style: &Style, width: usize) -> Vec<Line> {
        if let Some(lines) = self.image_block(content, width) {
            return lines;
        }
        let mut spans = Vec::new();
        self.inlines(content, style, &mut spans);
        wrap(&spans, width).into_iter().map(Line::new).collect()
    }

    /// A paragraph of only an image, or of only a link around an image,
    /// shown as the image itself when the terminal can and it loaded.
    /// The image inside a link is not clickable: OSC 8 around placeholders
    /// is not written yet.
    fn image_block(&self, content: &[Inline], width: usize) -> Option<Vec<Line>> {
        if !self.caps.draws_images() {
            return None;
        }
        let lone = Inline::lone_image(content)?;
        let url = &lone.url;
        let Some(Ok(source)) = self.images.get(url) else {
            return None;
        };
        let max_cells = self.caps.max_image_cells;
        let max_columns = u16::try_from(width).unwrap_or(u16::MAX).min(max_cells);
        let wanted = image::Wanted {
            width: lone.width,
            height: lone.height,
        };
        let size = image::size(
            source.size(),
            wanted,
            image::cell_size(self.caps),
            max_columns,
            max_cells,
        )?;
        let placement = Rc::new(Placement {
            url: url.to_owned(),
            size,
        });
        Some(
            (0..size.rows)
                .map(|row| Line::image(Rc::clone(&placement), row))
                .collect(),
        )
    }

    fn table(&self, table: &Table, width: usize) -> Vec<Line> {
        let header_style = self.style(Style {
            bold: true,
            ..Style::default()
        });
        let spans = |content: &[Inline], style: &Style| {
            let mut spans = Vec::new();
            self.inlines(content, style, &mut spans);
            spans
        };
        let cells = Cells {
            header: table
                .header
                .iter()
                .map(|c| spans(c, &header_style))
                .collect(),
            rows: (table.rows.iter())
                .map(|row| row.iter().map(|c| spans(c, &Style::default())).collect())
                .collect(),
            alignments: table.alignments.clone(),
        };
        let styles = table::Styles {
            border: self.style(Style::fg(self.theme.table_border)),
            header: header_style,
        };
        table::layout(&cells, width, &styles)
    }

    fn rule(&self, width: usize) -> Line {
        Line::new(vec![Span::new(
            &"─".repeat(width),
            self.style(Style::fg(self.theme.rule)),
        )])
    }

    fn inlines(&self, inlines: &[Inline], style: &Style, out: &mut Vec<Span>) {
        for inline in inlines {
            self.inline(inline, style, out);
        }
    }

    fn inline(&self, inline: &Inline, style: &Style, out: &mut Vec<Span>) {
        match inline {
            Inline::Text(text) => push(out, text, style),
            Inline::Code(code) => self.code(style, out, |style, out| push(out, code, style)),
            // A key is drawn as inline code is.
            Inline::Kbd(content) => {
                self.code(style, out, |style, out| self.inlines(content, style, out))
            }
            Inline::Emphasis(content) => self.inlines(
                content,
                &self.style(Style {
                    italic: true,
                    ..style.clone()
                }),
                out,
            ),
            Inline::Strong(content) => self.inlines(
                content,
                &self.style(Style {
                    bold: true,
                    ..style.clone()
                }),
                out,
            ),
            Inline::Strikethrough(content) => self.inlines(
                content,
                &self.style(Style {
                    strike: true,
                    ..style.clone()
                }),
                out,
            ),
            Inline::Mark(content) => {
                let mark = self.theme.mark;
                let style = if self.caps.color {
                    Style {
                        fg: mark.foreground.or(style.fg),
                        bg: mark.background.or(style.bg),
                        ..style.clone()
                    }
                } else {
                    Style {
                        reverse: true,
                        ..style.clone()
                    }
                };
                self.inlines(content, &self.style(style), out)
            }
            Inline::Link { url, content } => self.link(url, content, style, out),
            Inline::Image(Image { alt, .. }) => {
                let image_style = self.muted(Style {
                    italic: true,
                    ..style.clone()
                });
                push(
                    out,
                    &format!("[image: {}]", Inline::plain_text(alt)),
                    &image_style,
                );
            }
            Inline::SoftBreak => push(out, " ", style),
            Inline::LineBreak => push(out, "\n", style),
            Inline::Html(html) => push(out, html, &self.muted(style.clone())),
        }
    }

    /// Inline code, or what is drawn as it: `content` in the inline code
    /// colours, padded, or between backticks without colour.
    fn code(
        &self,
        style: &Style,
        out: &mut Vec<Span>,
        content: impl FnOnce(&Style, &mut Vec<Span>),
    ) {
        // Spaces in code are kept as written, so it copies as it is.
        let spaces = if self.theme.inline_code_wrap {
            Spaces::Keep
        } else {
            Spaces::KeepTogether
        };
        let style = &Style {
            spaces,
            ..style.clone()
        };
        // Without colour nothing else would mark the code.
        if self.caps.styling && self.caps.color {
            let bg = self.inline_code_background().or(style.bg);
            let code_style = Style {
                fg: self.theme.inline_code.or(style.fg),
                bg,
                ..style.clone()
            };
            // No-break spaces keep the padding attached to the code.
            let padding = if self.theme.inline_code_padding && bg.is_some() {
                "\u{a0}"
            } else {
                ""
            };
            push(out, padding, &code_style);
            content(&code_style, out);
            push(out, padding, &code_style);
        } else {
            push(out, "`", style);
            content(style, out);
            push(out, "`", style);
        }
    }

    fn link(&self, url: &str, content: &[Inline], style: &Style, out: &mut Vec<Span>) {
        let link_style = self.style(Style {
            fg: self.theme.link.or(style.fg),
            underline: Underline::Single,
            link: self.caps.hyperlinks.then(|| Rc::from(url)),
            ..style.clone()
        });
        self.inlines(content, &link_style, out);
        // Without clickable links, the target has to be visible.
        if !self.caps.hyperlinks && Inline::plain_text(content) != url {
            let target_style = self.muted(style.clone());
            push(out, &format!(" <{url}>"), &target_style);
        }
    }

    /// Text played down: in the theme's muted colour, or dimmed when the
    /// theme has none.
    fn muted(&self, style: Style) -> Style {
        self.style(match self.theme.muted {
            Some(color) => Style {
                fg: Some(color),
                ..style
            },
            None => Style { dim: true, ..style },
        })
    }

    /// Drops all styling when the terminal gets plain text.
    fn style(&self, style: Style) -> Style {
        if self.caps.styling {
            style
        } else {
            Style::default()
        }
    }
}
