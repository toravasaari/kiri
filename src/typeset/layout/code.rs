//! Code blocks and the colours code takes from its highlighting theme.

use unicode_width::UnicodeWidthStr;

use crate::typeset::block::BlockId;
use crate::typeset::color::Color;
use crate::typeset::driver::Stack;
use crate::typeset::images::ImageKey;
use crate::typeset::layout::highlight;
use crate::typeset::layout::{Cells, Layout, stamp, with_margin};
use crate::typeset::model::Block;
use crate::typeset::stretch::Stretch;
use crate::typeset::style::{Line, Origin, Origins, Span, Style, push};
use crate::typeset::text::{self, Part};
use crate::typeset::theme::LabelColor;
use crate::typeset::wrap;

/// What a code block keeps from one of its stretches to the next: how it is
/// drawn, which is fixed before the first stretch, and the state of its
/// highlighting.
pub(super) struct CodeRun {
    /// The highlighter of a block that is highlighted: with a box if it has
    /// padding, whatever happens to the highlighting later.
    highlighter: Option<highlight::Highlighter>,
    /// Whether the block has a box: a label row, a bottom row and a cell of
    /// padding on each side.
    padding: bool,
    /// The background of the box, which a whole file does not have.
    background: Option<Color>,
    /// The number of the block's text, that its rows show.
    text: Option<u32>,
}

/// The code block whose stretches the pen is laying out, between its first
/// stretch and its last.
#[derive(Default)]
pub(super) struct Stretching(Option<Open>);

struct Open {
    /// The block's one entry in the table of blocks.
    id: BlockId,
    run: CodeRun,
    /// The rows of the stretches laid out so far.
    rows: usize,
}

/// Where a code block's stretch is among the stretches.
pub(super) struct Ends {
    pub first: bool,
    pub last: bool,
}

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
        let text = self.cursor.get().text();
        let mut run = self.code_run(language, whole_file, text);
        let ends = Ends {
            first: true,
            last: true,
        };
        self.code_stretch(&mut run, language, (code, 0), ends, width)
    }

    /// How a code block in `language` is drawn, and where its highlighting
    /// starts. `text` is the number of its text, when the block shows the
    /// document's.
    pub(super) fn code_run(
        &self,
        language: Option<&str>,
        whole_file: bool,
        text: Option<u32>,
    ) -> CodeRun {
        let highlighter = (self.highlights_code() && !(whole_file && language.is_none()))
            .then(|| highlight::Highlighter::new(language, self.code_theme()))
            .flatten();
        CodeRun {
            background: (highlighter.as_ref()).and_then(|h| h.background().filter(|_| !whole_file)),
            highlighter,
            padding: self.look.code.padding && !whole_file,
            text,
        }
    }

    /// The lines of a stretch of a code block `width` cells wide: `code`, the
    /// whole lines of the block that start `offset` bytes in. Only the first
    /// stretch has the label row of a box and only the last its bottom row.
    pub(super) fn code_stretch(
        &self,
        run: &mut CodeRun,
        language: Option<&str>,
        (code, offset): (&str, usize),
        ends: Ends,
        width: usize,
    ) -> Vec<Line> {
        let rows = code_rows(code, run.text, offset);
        let Some(highlighter) = &mut run.highlighter else {
            let indent = if run.padding {
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
        let shown: String = rows.iter().map(|row| row.text.clone() + "\n").collect();
        let highlighted = highlighter.feed(&shown);
        let (background, padding) = (run.background, run.padding);

        // A box in the theme's background colour, one cell of padding on
        // each side and one line above and below.
        let background_style = Style {
            bg: background,
            ..Style::default()
        };
        let padded = |content: Vec<Span>| {
            let used = wrap::width(&content);
            let mut spans = vec![Span::new(" ", background_style.clone())];
            spans.extend(content);
            push(
                &mut spans,
                &" ".repeat(width.saturating_sub(used + 1)),
                &background_style,
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
        if padding && ends.first {
            let label = language
                .filter(|_| self.look.code.show_language_label)
                .and_then(|language| self.language_label(language, background, width));
            lines.push(label.unwrap_or_else(|| padded(Vec::new())));
        }
        for (source_line, row) in highlighted.into_iter().zip(&rows) {
            let source_line = row.give(source_line);
            let with_background = on_background(source_line, background);
            for chunk in wrap::split(&with_background, inner.max(1)) {
                lines.push(padded(chunk));
            }
        }
        if padding && ends.last {
            lines.push(padded(Vec::new()));
        }
        lines
    }

    /// What goes after the last line of a code block `width` cells wide: why
    /// it was not drawn as a diagram, and the band that tells that it is
    /// loading. `before` is the number of lines the block has above `lines`,
    /// which are those of its last stretch.
    pub(super) fn code_end(
        &self,
        (language, code): (Option<&str>, &str),
        width: usize,
        before: usize,
        lines: &mut Vec<Line>,
    ) {
        lines.extend(self.render_error(language, code, width));
        if let Some(language) = language {
            let key = ImageKey::diagram(language, code, self.looks.diagram, self.keys);
            self.loading_band(&key, width, lines);
            // The band covers the lines of the block above it too.
            if let Some(loading) = (lines.last_mut()).and_then(|line| line.loading.as_mut()) {
                loading.above += before;
            }
        }
    }

    /// The top padding line of a code block `width` cells wide, with
    /// `language` at its right end, one cell from the edge as the code is
    /// at the left. `None` when the name does not fit with a cell on each
    /// side.
    fn language_label(
        &self,
        language: &str,
        box_background: Option<Color>,
        width: usize,
    ) -> Option<Line> {
        let background = Style {
            bg: box_background,
            ..Style::default()
        };
        let label = Style {
            fg: match self.look.code.language_label {
                LabelColor::CodeBlock => {
                    let text = highlight::foreground(self.code_theme()).flatten();
                    halfway(text, box_background)
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

impl Cells<'_> {
    /// Whether the pen lays `block` out in stretches when it is large: a code
    /// block is, unless it is drawn as a diagram, which is never cut.
    pub(super) fn cuts(&self, block: &Block) -> bool {
        let Block::CodeBlock { language, code, .. } = block else {
            return false;
        };
        let layout = self.layout;
        layout
            .diagram(language.as_deref(), code, self.inner)
            .is_none()
    }

    /// Lays `stretch` out and stacks it, joined to the stretch before it.
    /// The block has one entry in the table, which its first stretch adds,
    /// and the stretches fix how it is drawn before the first.
    pub(super) fn stretch(&mut self, stretch: Stretch, path: Vec<usize>, stack: &mut Stack) {
        let Stretch {
            block, first, last, ..
        } = stretch;
        let Block::CodeBlock {
            language,
            code,
            whole_file,
        } = block
        else {
            return;
        };
        let layout = self.layout;
        let language = language.as_deref();
        if first {
            layout.marks.borrow_mut().path = path;
            let id = layout.add_block();
            let text = text::number(block, Part::Source).map(|text| text as u32);
            let run = layout.code_run(language, *whole_file, text);
            self.stretching.0 = Some(Open { id, run, rows: 0 });
        }
        let Some(open) = &mut self.stretching.0 else {
            return;
        };
        let ends = Ends { first, last };
        let shown = (stretch.code, stretch.offset);
        let mut lines = layout.code_stretch(&mut open.run, language, shown, ends, self.inner);
        if last {
            layout.code_end((language, code), self.inner, open.rows, &mut lines);
        }
        let mut lines = with_margin(lines, self.lead);
        stamp(&mut lines, open.id);
        open.rows += lines.len();
        match first {
            true => stack.push(lines),
            false => stack.join(lines),
        }
        if last {
            self.stretching.0 = None;
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
    /// Where the line ending is that follows the line (`\n` or `\r\n`) in the
    /// text of the code; empty for a last line that has none.
    pub(crate) ending: std::ops::Range<u32>,
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
/// not the document's text. `code` may be a stretch of the block's code
/// that starts `offset` bytes in.
pub(crate) fn code_rows(code: &str, text: Option<u32>, offset: usize) -> Vec<Row> {
    let mut start = 0;
    code.split_inclusive('\n')
        .map(|raw| {
            let line = raw.strip_suffix('\n').unwrap_or(raw);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let (text_of_row, mut origins) = expand_tabs_with(line);
            let first = (offset + start) as u32;
            let line_end = first + line.len() as u32;
            start += raw.len();
            let ending = line_end..(offset + start) as u32;
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
                ending,
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
