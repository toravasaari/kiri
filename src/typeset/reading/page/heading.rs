//! Headings: their text in the heading's size and colour, with a rule below
//! the first two levels.

use crate::typeset::model::Inline;
use crate::typeset::reading::page::{Flow, runs};
use crate::typeset::reading::text;

impl Flow<'_> {
    /// Lays a heading of `level` out at `x`, `width` pixels wide.
    pub(super) fn heading(&mut self, level: &u8, content: &[Inline], x: f32, width: f32) {
        let look = *self.look;
        let size = look.size;
        let index = usize::from(level.clamp(&1, &6) - 1);
        let style = text::Look {
            size: size * look.spacing.heading_sizes[index],
            line_height: look.spacing.heading_line_height,
            weight: look.spacing.bold,
            color: look.headings[index],
            ..text::Look::PLAIN
        };
        let ink = self.ink(width);
        let paragraph = text::lay_out(self.fonts, &runs(content, &look, ink), &style, width);
        self.y += self.text(paragraph, x);
        if *level <= look.spacing.heading_rules {
            self.y += (look.spacing.rule_below * style.size).round();
            self.rect(x, self.y, width, 1.0, look.border);
            self.y += 1.0;
        }
    }
}
