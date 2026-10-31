//! Headings: their text in the heading's size and colour, with a rule below
//! the first two levels.

use crate::typeset::model::Inline;
use crate::typeset::reading::page::{
    BOLD, Flow, HEADING_LINE_HEIGHT, HEADING_SIZES, RULE_BELOW, runs,
};
use crate::typeset::reading::text;

impl Flow<'_> {
    /// Lays a heading of `level` out at `x`, `width` pixels wide.
    pub(super) fn heading(&mut self, level: &u8, content: &[Inline], x: f32, width: f32) {
        let look = *self.look;
        let size = look.size;
        let index = usize::from(level.clamp(&1, &6) - 1);
        let style = text::Look {
            size: size * HEADING_SIZES[index],
            line_height: HEADING_LINE_HEIGHT,
            weight: BOLD,
            color: look.headings[index],
            ..text::Look::PLAIN
        };
        let ink = self.ink(width);
        let paragraph = text::lay_out(self.fonts, &runs(content, &look, ink), &style, width);
        self.y += self.text(paragraph, x);
        if *level <= 2 {
            self.y += (RULE_BELOW * style.size).round();
            self.rect(x, self.y, width, 1.0, look.border);
            self.y += 1.0;
        }
    }
}
