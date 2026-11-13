//! Details: the summary of a `<details>` block, and the blocks under it.

use crate::typeset::model::{Block, Inline};
use crate::typeset::reading::page::{Flow, push, runs_into};
use crate::typeset::reading::text::{self, Run};

impl Flow<'_> {
    /// Lays a `<details>` block out at `x`: its summary, then its blocks.
    pub(super) fn details(
        &mut self,
        summary: &[Inline],
        blocks: &[Block],
        x: f32,
        width: f32,
        depth: usize,
    ) {
        self.summary(summary, x, width);
        self.blocks(blocks, x, width, depth);
    }

    /// Lays the summary of a `<details>` out in bold after a triangle, as
    /// a browser shows an open one.
    fn summary(&mut self, summary: &[Inline], x: f32, width: f32) {
        self.text_is(Some(0));
        let style = text::Look {
            size: self.look.size,
            line_height: self.look.spacing.line_height,
            weight: self.look.spacing.bold,
            color: self.look.text,
            ..text::Look::PLAIN
        };
        let ink = self.ink(width);
        // The triangle is the layout's, and joins the text after it.
        let mut content = Vec::new();
        push(&mut content, "▼ ", &Run::default());
        runs_into(&mut content, summary, self.look, ink);
        let paragraph = text::lay_out(self.fonts, &content, &style, width);
        self.y += self.text(paragraph, x);
        self.margin = Some(self.look.size);
    }
}
