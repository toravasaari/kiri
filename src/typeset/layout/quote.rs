//! Block quotes and GitHub alerts.

use crate::typeset::color::Color;
use crate::typeset::layout::{Context, Layout};
use crate::typeset::model::{AlertKind, Block};
use crate::typeset::style::{Line, Span, Style, push};
use crate::typeset::theme::{CodeBackground, Theme};
use crate::typeset::wrap::{width as width_of, wrap};

impl Layout<'_> {
    pub(super) fn quote(&self, blocks: &[Block], width: usize, context: Context) -> Vec<Line> {
        let bar = if self.caps.styling { "│ " } else { "> " };
        let bar_style = self.style(Style::fg(self.theme.quote_bar));
        let inner = Context {
            nested: true,
            tight: false,
            ..context
        };
        let mut lines = self.blocks(blocks, width.saturating_sub(2).max(1), inner);
        for line in &mut lines {
            // A blank line gets the bar without the space after it.
            let bar = if line.is_blank() { bar.trim_end() } else { bar };
            line.prepend(Span::new(bar, bar_style.clone()));
        }
        lines
    }

    /// A GitHub alert: a bar and a title in the kind's colour over a tint
    /// of it. Without styling, a quote with the title on its first line.
    #[expect(clippy::too_many_lines)]
    pub(super) fn alert(
        &self,
        kind: AlertKind,
        title: Option<&str>,
        blocks: &[Block],
        width: usize,
        context: Context,
    ) -> Vec<Line> {
        let title = title.unwrap_or(kind.name());
        let inner = Context {
            nested: true,
            tight: false,
            ..context
        };
        let body_width = width.saturating_sub(2).max(1);
        if !self.caps.styling {
            let title = [Span::new(title, Style::default())];
            let mut lines: Vec<Line> = (wrap(&title, body_width).into_iter())
                .map(Line::new)
                .collect();
            lines.extend(self.blocks(blocks, body_width, inner));
            for line in &mut lines {
                let bar = if line.is_blank() { ">" } else { "> " };
                line.prepend(Span::new(bar, Style::default()));
            }
            return lines;
        }

        let dark = self.dark();
        let color = self.theme.alerts.color(kind, dark);
        let fill = Style {
            bg: self.tint(kind, color, false),
            ..Style::default()
        };
        // Inline code inside stands out with a stronger tint, unless the
        // theme gives inline code no background.
        let theme;
        let code =
            (self.tint(kind, color, true)).filter(|_| self.inline_code_background().is_some());
        // GitHub's dark yellow reads as brown on its pale yellow: a warning
        // there has a gold bar and a darker gold title.
        let (bar_color, color) = match self.theme.alerts.github_gold(kind, dark) {
            Some((bar, title)) => (Some(bar), Some(title)),
            None => (color, color),
        };
        let body = match code {
            Some(code) => {
                theme = Theme {
                    inline_code_background: CodeBackground::Color(Some(code)),
                    ..self.theme.clone()
                };
                Layout {
                    theme: &theme,
                    ..*self
                }
            }
            None => Layout { ..*self },
        };

        // kitty draws an Octicon from the symbols font it bundles over two
        // cells, covering the first space after it.
        let icon = if self.caps.text_sizing {
            let style = Style {
                fg: color,
                ..fill.clone()
            };
            vec![
                Span::new(octicon(kind), style),
                Span::new("  ", fill.clone()),
            ]
        } else {
            Vec::new()
        };
        let indent = width_of(&icon);
        let strong = Style {
            fg: color,
            bold: true,
            ..fill.clone()
        };
        // A long title wraps under its own text, past the icon.
        let title_width = body_width.saturating_sub(indent).max(1);
        let title = [Span::new(title, strong)];
        let mut lines: Vec<Line> = (wrap(&title, title_width).into_iter().enumerate())
            .map(|(row, text)| {
                let mut spans = if row == 0 {
                    icon.clone()
                } else {
                    vec![Span::new(&" ".repeat(indent), fill.clone())]
                };
                spans.extend(text);
                Line::new(spans)
            })
            .collect();
        lines.extend(body.blocks(blocks, body_width, inner));
        // A blank row of the tint under the text, as GitHub pads an alert.
        if fill.bg.is_some() {
            lines.push(Line::new(Vec::new()));
        }

        let bar = Style {
            fg: bar_color,
            ..fill.clone()
        };
        for line in &mut lines {
            if fill.bg.is_some() {
                for span in &mut line.spans {
                    span.style.bg = span.style.bg.or(fill.bg);
                }
                if let Some(image) = &mut line.image {
                    image.background = image.background.or(fill.bg);
                }
                let image = (line.image.as_ref()).map_or(0, |row| row.columns.len());
                let used = width_of(&line.spans) + image;
                push(
                    &mut line.trail,
                    &" ".repeat(body_width.saturating_sub(used)),
                    &fill,
                );
            }
            if !line.is_blank() || fill.bg.is_some() {
                line.prepend(Span::new(" ", fill.clone()));
            }
            line.prepend(Span::new("▎", bar.clone()));
        }
        lines
    }

    /// The background of an alert of `kind`, or with `strong` that of
    /// inline code in it. GitHub's own colours on a light page have its
    /// pale backgrounds, which a mix with the page does not match: dark
    /// yellow and white make beige. They tint the page as a colour filter
    /// does, and inline code takes the filter twice. Otherwise 15 or 30 %
    /// of `color` is mixed into the page. `None` when a colour is not
    /// known as RGB, or the theme has no tints.
    fn tint(&self, kind: AlertKind, color: Option<Color>, strong: bool) -> Option<Color> {
        if !self.theme.alerts.tint {
            return None;
        }
        match self.theme.alerts.github_background(kind, self.dark()) {
            Some(pale) => {
                let once = self.page()?.filter(pale)?;
                if strong {
                    once.filter(pale)
                } else {
                    Some(once)
                }
            }
            None => self.mixed_into_page(color, if strong { 0.3 } else { 0.15 }),
        }
    }
}

/// The Octicon GitHub shows for `kind`, in the Nerd Font symbols kitty
/// bundles since 0.36.
fn octicon(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::Note => "\u{f449}",
        AlertKind::Tip => "\u{f400}",
        AlertKind::Important => "\u{f50a}",
        AlertKind::Warning => "\u{f421}",
        AlertKind::Caution => "\u{f46e}",
    }
}
