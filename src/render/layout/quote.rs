//! Block quotes and GitHub alerts.

use super::{Context, Layout};
use crate::color::Color;
use crate::model::{AlertKind, Block};
use crate::render::style::{Line, Span, Style, push};
use crate::render::wrap::width as width_of;
use crate::theme::{CodeBackground, Theme};

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
            let mut lines = vec![Line::new(vec![Span::new(title, Style::default())])];
            lines.extend(self.blocks(blocks, body_width, inner));
            for line in &mut lines {
                let bar = if line.is_blank() { ">" } else { "> " };
                line.prepend(Span::new(bar, Style::default()));
            }
            return lines;
        }

        let dark = self.caps.dark_background.unwrap_or(true);
        let color = self.theme.alerts.color(kind, dark);
        let tint = |amount| self.tint(color, amount);
        let fill = Style {
            bg: tint(0.15),
            ..Style::default()
        };
        // Inline code inside stands out with a stronger tint.
        let theme;
        let body = match tint(0.3) {
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

        let mut heading = Vec::new();
        if self.caps.text_sizing {
            // kitty draws an Octicon from the symbols font it bundles over
            // two cells, covering the first space after it.
            let icon = Style {
                fg: color,
                ..fill.clone()
            };
            push(&mut heading, octicon(kind), &icon);
            push(&mut heading, "  ", &fill);
        }
        let strong = Style {
            fg: color,
            bold: true,
            ..fill.clone()
        };
        push(&mut heading, title, &strong);
        let mut lines = vec![Line::new(heading)];
        lines.extend(body.blocks(blocks, body_width, inner));

        let bar = Style {
            fg: color,
            ..fill.clone()
        };
        for line in &mut lines {
            if fill.bg.is_some() {
                for span in &mut line.spans {
                    span.style.bg = span.style.bg.or(fill.bg);
                }
                let image = (line.image.as_ref()).map_or(0, |row| row.placement.size.columns);
                let used = width_of(&line.spans) + usize::from(image);
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

    /// `amount` of `color` mixed into the page: the theme's background, or
    /// else the terminal's. `None` when either is not known as RGB, or the
    /// theme has no tints.
    fn tint(&self, color: Option<Color>, amount: f32) -> Option<Color> {
        if !self.theme.alerts.tint {
            return None;
        }
        let page = self.theme.background.or(self.caps.background);
        self.rgb(page?)?.mix(self.rgb(color?)?, amount)
    }

    /// `color` as RGB, a palette colour as the terminal told it.
    fn rgb(&self, color: Color) -> Option<Color> {
        match color {
            Color::Rgb(..) => Some(color),
            Color::Palette(index) => *self.caps.palette.get(usize::from(index))?,
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
