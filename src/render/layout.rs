//! Document model to styled lines of a fixed width.

use std::rc::Rc;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::highlight;
use super::style::{Line, Spaces, Span, Style, push};
use super::table::{self, Cells};
use super::wrap::wrap;
use crate::color::{Color, Underline};
use crate::model::{Block, Document, Inline, List, Table};
use crate::term::Capabilities;
use crate::theme::{CodeBackground, Theme};

const BULLETS: [&str; 3] = ["•", "◦", "▪"];

pub fn layout(document: &Document, caps: &Capabilities, theme: &Theme) -> Vec<Line> {
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
    let layout = Layout { caps, theme };

    // Headings stay at the edge and thematic breaks span the terminal;
    // everything else keeps text_margin free on both sides.
    let margin = usize::from(theme.text_margin).min(width / 4);
    let mut lines = Vec::new();
    for block in &document.blocks {
        let block_lines = match block {
            Block::ThematicBreak => vec![layout.rule(usize::from(caps.width).max(width))],
            Block::Heading { .. } => layout.block(block, width, Context::default()),
            _ => {
                let mut block_lines = layout.block(block, width - 2 * margin, Context::default());
                for line in &mut block_lines {
                    if margin > 0 && !line.spans.is_empty() {
                        line.lead
                            .insert(0, Span::new(&" ".repeat(margin), Style::default()));
                    }
                }
                block_lines
            }
        };
        append_block(&mut lines, block_lines, true);
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
    caps: &'a Capabilities,
    theme: &'a Theme,
}

impl Layout<'_> {
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
            Block::List(list) => self.list(list, width, context),
            Block::Table(table) => self.table(table, width),
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

    fn paragraph(&self, content: &[Inline], style: &Style, width: usize) -> Vec<Line> {
        let mut spans = Vec::new();
        self.inlines(content, style, &mut spans);
        wrap(&spans, width).into_iter().map(Line::new).collect()
    }

    fn code_block(&self, language: Option<&str>, code: &str, width: usize) -> Vec<Line> {
        let code: String = code.lines().map(|line| expand_tabs(line) + "\n").collect();
        // Highlighting is all colour, down to the box around the code.
        let highlighted = if self.caps.styling && self.caps.color {
            highlight::highlight(&code, language, self.code_theme())
        } else {
            None
        };
        let padding = self.theme.code_padding;
        let Some(highlighted) = highlighted else {
            let indent = if padding { "  " } else { "" };
            return code
                .lines()
                .flat_map(|line| {
                    let max = width.saturating_sub(indent.len()).max(1);
                    split_to_width(&[Span::new(line, Style::default())], max)
                })
                .map(|mut spans| {
                    spans.insert(0, Span::new(indent, Style::default()));
                    Line::new(spans)
                })
                .collect();
        };

        // A box in the theme's background colour, one cell of padding on
        // each side and one line above and below.
        let background = Style {
            bg: highlighted.background,
            ..Style::default()
        };
        let padded = |content: Vec<Span>| {
            let used = super::wrap::width(&content);
            let mut spans = vec![Span::new(" ", background.clone())];
            spans.extend(content);
            push(
                &mut spans,
                &" ".repeat(width.saturating_sub(used + 1)),
                &background,
            );
            Line::new(spans)
        };
        // Without padding the code starts at the margin and nothing but the
        // code itself is copied along with it.
        let padded = |content: Vec<Span>| {
            if padding {
                padded(content)
            } else {
                Line::new(content)
            }
        };
        let inner = if padding {
            width.saturating_sub(2)
        } else {
            width
        };
        let mut lines = Vec::new();
        if padding {
            lines.push(padded(Vec::new()));
        }
        for source_line in highlighted.lines {
            let with_background: Vec<Span> = source_line
                .into_iter()
                .map(|span| Span {
                    style: Style {
                        bg: highlighted.background,
                        ..span.style
                    },
                    ..span
                })
                .collect();
            for chunk in split_to_width(&with_background, inner.max(1)) {
                lines.push(padded(chunk));
            }
        }
        if padding {
            lines.push(padded(Vec::new()));
        }
        lines
    }

    fn inline_code_background(&self) -> Option<Color> {
        match self.theme.inline_code_background {
            CodeBackground::CodeBlock => highlight::background(self.code_theme()).flatten(),
            CodeBackground::Color(color) => color,
        }
    }

    fn code_theme(&self) -> &str {
        if self.caps.dark_background.unwrap_or(true) {
            &self.theme.code_theme_dark
        } else {
            &self.theme.code_theme_light
        }
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
        table::layout(
            &cells,
            width,
            &self.style(Style::fg(self.theme.table_border)),
        )
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
                    if self.theme.inline_code_padding && bg.is_some() {
                        // No-break spaces keep the padding attached to the code.
                        push(out, &format!("\u{a0}{code}\u{a0}"), &code_style);
                    } else {
                        push(out, code, &code_style);
                    }
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
            underline: Underline::Single,
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

/// Splits styled code into lines of at most `max` cells, keeping every
/// character, spaces included.
fn split_to_width(spans: &[Span], max: usize) -> Vec<Vec<Span>> {
    let mut lines = vec![Vec::new()];
    let mut used = 0;
    for span in spans {
        for c in span.text.chars() {
            let char_width = c.width().unwrap_or(0);
            if used + char_width > max && used > 0 {
                lines.push(Vec::new());
                used = 0;
            }
            let line = lines.last_mut().expect("lines is never empty");
            push(line, c.encode_utf8(&mut [0; 4]), &span.style);
            used += char_width;
        }
    }
    lines
}
