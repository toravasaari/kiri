//! Document model to styled lines of a fixed width.

use std::rc::Rc;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::style::{Line, Span, Style, push};
use super::wrap::wrap;
use crate::model::{Block, Document, Inline, List};
use crate::term::Capabilities;
use crate::theme::Theme;

const BULLETS: [&str; 3] = ["•", "◦", "▪"];

pub fn layout(document: &Document, caps: &Capabilities, theme: &Theme) -> Vec<Line> {
    let mut width = usize::from(caps.width);
    if theme.max_width > 0 {
        width = width.min(usize::from(theme.max_width));
    }
    let layout = Layout { caps, theme };
    layout.blocks(&document.blocks, width.max(10), Context::default())
}

#[derive(Debug, Clone, Copy, Default)]
struct Context {
    /// Inside a quote or list, where scaled headings would break the prefix.
    nested: bool,
    list_depth: usize,
    /// Tight list item: no blank lines between its blocks.
    tight: bool,
}

struct Layout<'a> {
    caps: &'a Capabilities,
    theme: &'a Theme,
}

impl Layout<'_> {
    fn blocks(&self, blocks: &[Block], width: usize, context: Context) -> Vec<Line> {
        let mut lines = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            if index > 0 && !context.tight {
                lines.push(Line::blank());
            }
            lines.extend(self.block(block, width, context));
        }
        lines
    }

    fn block(&self, block: &Block, width: usize, context: Context) -> Vec<Line> {
        match block {
            Block::Heading { level, content } => self.heading(*level, content, width, context),
            Block::Paragraph(content) => self.paragraph(content, &Style::default(), width),
            Block::CodeBlock { code, .. } => self.code_block(code, width),
            Block::BlockQuote(blocks) => self.quote(blocks, width, context),
            Block::List(list) => self.list(list, width, context),
            Block::Table(_) => Vec::new(),
            Block::ThematicBreak => vec![self.rule(width)],
            Block::Html(html) => {
                let style = self.style(Style {
                    dim: true,
                    ..Style::default()
                });
                html.lines()
                    .map(|line| Line::new(vec![Span::new(line, style.clone())]))
                    .collect()
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
            fg: heading.color,
            ..Style::default()
        });
        let mut spans = Vec::new();
        if !self.caps.styling {
            push(
                &mut spans,
                &format!("{} ", "#".repeat(usize::from(level))),
                &style,
            );
        }
        self.inlines(content, &style, &mut spans);
        let mut lines: Vec<Line> = wrap(&spans, width / usize::from(scale))
            .into_iter()
            .map(|spans| Line { spans, scale })
            .collect();
        if heading.rule && self.caps.styling {
            lines.push(self.rule(width));
        }
        lines
    }

    fn paragraph(&self, content: &[Inline], style: &Style, width: usize) -> Vec<Line> {
        let mut spans = Vec::new();
        self.inlines(content, style, &mut spans);
        wrap(&spans, width).into_iter().map(Line::new).collect()
    }

    fn code_block(&self, code: &str, width: usize) -> Vec<Line> {
        let indent = "  ";
        let inner = width.saturating_sub(indent.len()).max(1);
        let mut lines = Vec::new();
        for source_line in code.lines() {
            let text = expand_tabs(source_line);
            for chunk in split_to_width(&text, inner) {
                lines.push(Line::new(vec![Span::new(
                    &format!("{indent}{chunk}"),
                    Style::default(),
                )]));
            }
        }
        lines
    }

    fn quote(&self, blocks: &[Block], width: usize, context: Context) -> Vec<Line> {
        let bar = if self.caps.styling { "│ " } else { "> " };
        let bar_style = self.style(Style::fg(self.theme.quote_bar));
        let inner = Context {
            nested: true,
            tight: false,
            ..context
        };
        let mut lines = self.blocks(blocks, width.saturating_sub(2).max(1), inner);
        for line in &mut lines {
            line.prepend(Span::new(bar, bar_style.clone()));
        }
        lines
    }

    fn list(&self, list: &List, width: usize, context: Context) -> Vec<Line> {
        // A styled bullet list shows a task's checkbox in place of the bullet.
        let boxes_as_markers = list.start.is_none() && self.caps.styling;
        let markers: Vec<String> = (list.items.iter().enumerate())
            .map(|(index, item)| match (list.start, item.checked) {
                (Some(start), _) => format!("{}.", start + index as u64),
                (None, Some(checked)) if boxes_as_markers => {
                    checkbox(checked, true).trim_end().into()
                }
                (None, _) if self.caps.styling => {
                    BULLETS[context.list_depth % BULLETS.len()].into()
                }
                (None, _) => "-".into(),
            })
            .collect();
        let marker_width = markers.iter().map(|m| m.width()).max().unwrap_or(0) + 1;
        let marker_style = self.style(Style::fg(self.theme.list_marker));
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
            for (line_index, line) in item_lines.iter_mut().enumerate() {
                if line_index == 0 {
                    for span in first.iter().rev() {
                        line.prepend(span.clone());
                    }
                } else if !line.spans.is_empty() {
                    line.prepend(Span::new(&" ".repeat(prefix_width), Style::default()));
                }
            }
            lines.extend(item_lines);
        }
        lines
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
            Inline::Code(code) => {
                if self.caps.styling {
                    let code_style = Style {
                        fg: self.theme.inline_code.or(style.fg),
                        ..style.clone()
                    };
                    push(out, code, &code_style);
                } else {
                    push(out, &format!("`{code}`"), style);
                }
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
            Inline::Link { url, content } => self.link(url, content, style, out),
            Inline::Image { alt, .. } => {
                let image_style = self.style(Style {
                    dim: true,
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
            Inline::Html(html) => push(
                out,
                html,
                &self.style(Style {
                    dim: true,
                    ..style.clone()
                }),
            ),
        }
    }

    fn link(&self, url: &str, content: &[Inline], style: &Style, out: &mut Vec<Span>) {
        let link_style = self.style(Style {
            fg: self.theme.link.or(style.fg),
            underline: true,
            link: self.caps.hyperlinks.then(|| Rc::from(url)),
            ..style.clone()
        });
        self.inlines(content, &link_style, out);
        // Without clickable links, the target has to be visible.
        if !self.caps.hyperlinks && Inline::plain_text(content) != url {
            let target_style = self.style(Style {
                dim: true,
                ..style.clone()
            });
            push(out, &format!(" <{url}>"), &target_style);
        }
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

fn checkbox(checked: bool, styled: bool) -> &'static str {
    match (checked, styled) {
        (true, true) => "☑ ",
        (false, true) => "☐ ",
        (true, false) => "[x] ",
        (false, false) => "[ ] ",
    }
}

fn expand_tabs(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars() {
        if c == '\t' {
            let spaces = 4 - out.width() % 4;
            out.push_str(&" ".repeat(spaces));
        } else {
            out.push(c);
        }
    }
    out
}

/// Splits code into pieces of at most `max` cells, keeping every character.
fn split_to_width(text: &str, max: usize) -> Vec<String> {
    let mut pieces = vec![String::new()];
    let mut used = 0;
    for c in text.chars() {
        let char_width = c.width().unwrap_or(0);
        if used + char_width > max && used > 0 {
            pieces.push(String::new());
            used = 0;
        }
        pieces.last_mut().expect("pieces is never empty").push(c);
        used += char_width;
    }
    pieces
}
