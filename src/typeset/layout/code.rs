//! Code blocks and the colours code takes from its highlighting theme.

use unicode_width::UnicodeWidthStr;

use crate::typeset::color::Color;
use crate::typeset::layout::Layout;
use crate::typeset::layout::highlight::{self, Highlighted};
use crate::typeset::style::{Line, Origin, Origins, Span, Style, push};
use crate::typeset::theme::LabelColor;
use crate::typeset::wrap;

impl Layout<'_> {
    /// `code` in `language` as a block `width` cells wide. A whole file
    /// shown as text, `whole_file`, has no box: no background, padding or
    /// language label, and the code starts at the margin. Without a
    /// language it is not highlighted, so that it keeps the terminal's own
    /// colours.
    pub(super) fn code_block(
        &self,
        language: Option<&str>,
        code: &str,
        whole_file: bool,
        width: usize,
    ) -> Vec<Line> {
        // What each line shows of the document's code, when it is shown.
        let shown = self.cursor.get().text();
        let rows = code_rows(code, shown);
        let code: String = rows.iter().map(|row| row.text.clone() + "\n").collect();
        let highlighted = if self.highlights_code() && !(whole_file && language.is_none()) {
            highlight::highlight(&code, language, self.code_theme())
        } else {
            None
        };
        let highlighted = highlighted.map(|highlighted| Highlighted {
            background: highlighted.background.filter(|_| !whole_file),
            ..highlighted
        });
        let padding = self.look.code.padding && !whole_file;
        let Some(highlighted) = highlighted else {
            let indent = if padding {
                " ".repeat(self.look.cells.plain_code_indent)
            } else {
                String::new()
            };
            return rows
                .iter()
                .flat_map(|row| {
                    let max = width.saturating_sub(indent.len()).max(1);
                    wrap::split(&[row.span(Style::default())], max)
                })
                .map(|mut spans| {
                    spans.insert(0, Span::new(&indent, Style::default()));
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
            let used = wrap::width(&content);
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
                .filter(|_| self.look.code.show_language_label)
                .and_then(|language| self.language_label(language, &highlighted, width));
            lines.push(label.unwrap_or_else(|| padded(Vec::new())));
        }
        for (source_line, row) in highlighted.lines.into_iter().zip(&rows) {
            let source_line = row.give(source_line);
            let with_background = on_background(source_line, highlighted.background);
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
            fg: match self.look.code.language_label {
                LabelColor::CodeBlock => {
                    let text = highlight::foreground(self.code_theme()).flatten();
                    halfway(text, highlighted.background)
                }
                LabelColor::Color(color) => color,
            },
            ..background.clone()
        };
        let name = [Span::new(language, label.clone())];
        let used = wrap::width(&name) + 2;
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
        self.look.inline_code.background
    }

    /// Whether code blocks are highlighted, and in a box when they have
    /// padding: highlighting is all colour, down to the box around the
    /// code, and needs a theme kiri has.
    fn highlights_code(&self) -> bool {
        self.caps.styling && self.caps.color && highlight::background(self.code_theme()).is_some()
    }

    /// How lines below a code block `width` cells wide, such as why it was
    /// not drawn as a diagram, line up with it: the cells before them, and
    /// the most cells they take after those. Below a box they start at its
    /// edge and are at most as wide as the code in it; below code without
    /// one, they start where the code does.
    pub(super) fn below_code(&self, width: usize) -> (usize, usize) {
        let padding = self.look.code.padding;
        let indent = match padding && !self.highlights_code() {
            true => self.look.cells.plain_code_indent,
            false => 0,
        };
        let inner = if padding {
            width.saturating_sub(2)
        } else {
            width
        };
        (indent, inner.max(1))
    }

    /// How reading mode colours code blocks, as this layout does.
    pub(super) fn code_colors(&self) -> CodeColors {
        let theme = self.highlights_code().then(|| self.code_theme().to_owned());
        let background = (theme.as_deref()).and_then(highlight::background).flatten();
        let text = highlight::foreground(self.code_theme()).flatten();
        let label = match self.look.code.language_label {
            _ if !self.look.code.show_language_label || !self.look.code.padding => None,
            LabelColor::CodeBlock => halfway(text, background),
            LabelColor::Color(color) => color,
        };
        CodeColors {
            theme,
            background,
            text,
            label,
        }
    }

    pub(super) fn code_theme(&self) -> &str {
        if self.dark() {
            &self.look.code.theme_dark
        } else {
            &self.look.code.theme_light
        }
    }
}

/// How code blocks are coloured: the highlighting theme, when code is
/// highlighted, the block's background and text colour, and the colour of
/// its language label, when it has one.
#[derive(Debug, Clone, Default)]
pub struct CodeColors {
    pub theme: Option<String>,
    pub background: Option<Color>,
    pub text: Option<Color>,
    pub label: Option<Color>,
}

/// The colour halfway between two RGB colours, rounded up; `a` when
/// either is missing or a palette colour, whose value is the terminal's.
/// A code theme without a text colour so leaves the label in the
/// terminal's; every bundled theme has one.
fn halfway(a: Option<Color>, b: Option<Color>) -> Option<Color> {
    let a = a?;
    Some(b.and_then(|b| a.mix(b, 0.5)).unwrap_or(a))
}

/// The spans of a highlighted `line` on the code's `background`, which
/// sets off the code from the page.
pub(super) fn on_background(line: Vec<Span>, background: Option<Color>) -> Vec<Span> {
    (line.into_iter())
        .map(|span| Span {
            style: Style {
                bg: background,
                ..span.style
            },
            ..span
        })
        .collect()
}

pub(crate) fn expand_tabs(line: &str) -> String {
    expand_tabs_with(line).0
}

/// `line` with its tabs spread, and what of `line` each part of the result
/// shows, as origins of text 0: the bytes between tabs as they are and each
/// spread tab as a whole.
fn expand_tabs_with(line: &str) -> (String, Vec<Origin>) {
    let mut out = String::new();
    let mut origins: Vec<Origin> = Vec::new();
    for (at, c) in line.char_indices() {
        let start = out.len();
        if c == '\t' {
            let spaces = 4 - out.width() % 4;
            out.push_str(&" ".repeat(spaces));
        } else {
            out.push(c);
        }
        let (shown, doc) = (
            start as u32..out.len() as u32,
            at as u32..(at + c.len_utf8()) as u32,
        );
        match origins.last_mut() {
            Some(last) if c != '\t' && last.exact && last.shown.end == shown.start => {
                last.shown.end = shown.end;
                last.doc.end = doc.end;
            }
            _ => origins.push(Origin {
                shown,
                doc,
                exact: c != '\t',
                ..Origin::exact(0, 0..0)
            }),
        }
    }
    (out, origins)
}

/// A line of code as it is shown, with its tabs spread, and what it shows of
/// the document's code.
pub(crate) struct Row {
    text: String,
    /// The text and the bytes in it that the row shows, when it shows any.
    origins: Vec<Origin>,
    /// Where the row's line is in the text of the code, and the number of
    /// the text, when the code is shown as it is.
    pub(crate) doc: Option<(u32, std::ops::Range<u32>)>,
}

impl Row {
    /// The row as a span in `style`.
    pub(super) fn span(&self, style: Style) -> Span {
        Span::new(&self.text, style).with_origins(self.origins.clone())
    }

    /// `spans`, which are the row cut in styled parts, with what each part
    /// shows. They are left as they are when they are not the row's text.
    pub(super) fn give(&self, spans: Vec<Span>) -> Vec<Span> {
        let lengths: Vec<usize> = spans.iter().map(|span| span.text.len()).collect();
        let mut origins = self.parts(&lengths).into_iter();
        spans
            .into_iter()
            .map(|span| match origins.next() {
                Some(origins) => Span { origins, ..span },
                None => span,
            })
            .collect()
    }

    /// What each of the parts of the row, `lengths` long, shows; none when
    /// the parts are not the row's text.
    pub(crate) fn parts(&self, lengths: &[usize]) -> Vec<Origins> {
        if lengths.iter().sum::<usize>() != self.text.len() {
            return Vec::new();
        }
        let whole = Origins::from_origins(self.origins.clone());
        let mut at = 0;
        (lengths.iter())
            .map(|length| {
                at += length;
                whole.part(at - length..at)
            })
            .collect()
    }
}

/// The rows of `code` shown in a code block. `text` is the number of the
/// text of the block that `code` is, or none when what the block shows is
/// not the document's text.
pub(crate) fn code_rows(code: &str, text: Option<u32>) -> Vec<Row> {
    let mut start = 0;
    code.split_inclusive('\n')
        .map(|raw| {
            let line = raw.strip_suffix('\n').unwrap_or(raw);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let (text_of_row, mut origins) = expand_tabs_with(line);
            let first = start as u32;
            let line_end = first + line.len() as u32;
            start += raw.len();
            match text {
                Some(text) => {
                    for origin in &mut origins {
                        origin.text = text;
                        origin.doc = origin.doc.start + first..origin.doc.end + first;
                    }
                }
                None => origins.clear(),
            }
            Row {
                text: text_of_row,
                origins,
                doc: text.map(|text| (text, first..line_end)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halfway_rounds_up_and_keeps_the_first_colour_it_cannot_mix() {
        let text = Color::Rgb(0xc0, 0xc5, 0xce);
        let page = Color::Rgb(0x2b, 0x30, 0x3b);
        let mid = Some(Color::Rgb(118, 123, 133));
        assert_eq!(halfway(Some(text), Some(page)), mid);
        assert_eq!(halfway(Some(page), Some(text)), mid);
        assert_eq!(halfway(Some(text), None), Some(text));
        assert_eq!(halfway(Some(text), Some(Color::Palette(0))), Some(text));
        let palette = Some(Color::Palette(7));
        assert_eq!(halfway(palette, Some(page)), palette);
        assert_eq!(halfway(None, Some(page)), None);
    }
}
