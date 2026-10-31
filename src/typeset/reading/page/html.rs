//! Raw HTML, which is shown as its source until there is an HTML backend.

use crate::typeset::reading::page::Flow;
use crate::typeset::reading::text::{self, Run};

impl Flow<'_> {
    /// Lays raw HTML out at `x`, `width` pixels wide, as its source.
    pub(super) fn html(&mut self, html: &str, x: f32, width: f32) {
        let look = *self.look;
        let size = look.size;
        // Source, as the text layout shows it: muted, in the code's
        // font and in lines of its own.
        let style = text::Look {
            size: size * text::CODE_SIZE,
            line_height: look.spacing.code_line_height,
            color: look.quote,
            mono: true,
            wrap: text::Wrap::Anywhere,
            ..text::Look::PLAIN
        };
        let paragraph = text::lay_out(self.fonts, &[Run::new(html)], &style, width);
        self.y += self.text(paragraph, x);
    }
}
