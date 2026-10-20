//! Code blocks and the colours code takes from its highlighting theme.

use unicode_width::UnicodeWidthStr;

use super::Layout;
use crate::color::Color;
use crate::render::highlight;
use crate::render::style::{Line, Span, Style, push};
use crate::render::wrap;
use crate::theme::{CodeBackground, LabelColor};

impl Layout<'_> {
    pub(super) fn code_block(&self, language: Option<&str>, code: &str, width: usize) -> Vec<Line> {
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
                    wrap::split(&[Span::new(line, Style::default())], max)
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
            let used = crate::render::wrap::width(&content);
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
            let label = language
                .filter(|_| self.theme.show_language_label)
                .and_then(|language| self.language_label(language, &highlighted, width));
            lines.push(label.unwrap_or_else(|| padded(Vec::new())));
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
            for chunk in wrap::split(&with_background, inner.max(1)) {
                lines.push(padded(chunk));
            }
        }
        if padding {
            lines.push(padded(Vec::new()));
        }
        lines
    }

    /// The top padding line of a code block `width` cells wide, with
    /// `language` at its right end, one cell from the edge as the code is
    /// at the left. `None` when the name does not fit with a cell on each
    /// side.
    fn language_label(
        &self,
        language: &str,
        highlighted: &highlight::Highlighted,
        width: usize,
    ) -> Option<Line> {
        let background = Style {
            bg: highlighted.background,
            ..Style::default()
        };
        let label = Style {
            fg: match self.theme.language_label {
                LabelColor::CodeBlock => {
                    let text = highlight::foreground(self.code_theme()).flatten();
                    halfway(text, highlighted.background)
                }
                LabelColor::Color(color) => color,
            },
            ..background.clone()
        };
        let name = [Span::new(language, label.clone())];
        let used = crate::render::wrap::width(&name) + 2;
        if used > width {
            return None;
        }
        let mut spans = Vec::new();
        push(&mut spans, &" ".repeat(width - used + 1), &background);
        push(&mut spans, language, &label);
        push(&mut spans, " ", &background);
        Some(Line::new(spans))
    }

    pub(super) fn inline_code_background(&self) -> Option<Color> {
        match self.theme.inline_code_background {
            CodeBackground::CodeBlock => highlight::background(self.code_theme()).flatten(),
            CodeBackground::Color(color) => color,
        }
    }

    pub(super) fn code_theme(&self) -> &str {
        if self.caps.dark_background.unwrap_or(true) {
            &self.theme.code_theme_dark
        } else {
            &self.theme.code_theme_light
        }
    }
}

/// The colour halfway between two RGB colours, rounded up; `a` when
/// either is missing or a palette colour, whose value is the terminal's.
/// A code theme without a text colour so leaves the label in the
/// terminal's; every bundled theme has one.
fn halfway(a: Option<Color>, b: Option<Color>) -> Option<Color> {
    match (a, b) {
        (Some(Color::Rgb(r1, g1, b1)), Some(Color::Rgb(r2, g2, b2))) => {
            let mid = |x: u8, y: u8| ((u16::from(x) + u16::from(y)).div_ceil(2)) as u8;
            Some(Color::Rgb(mid(r1, r2), mid(g1, g2), mid(b1, b2)))
        }
        _ => a,
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
