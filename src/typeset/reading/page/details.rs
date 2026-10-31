//! Details: the summary of a `<details>` block, and the blocks under it.

use crate::typeset::model::{Block, Inline};
use crate::typeset::reading::page::{BOLD, Flow, LINE_HEIGHT, runs};
use crate::typeset::reading::text;

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
        let mut content = vec![Inline::Text("▼ ".to_owned())];
        content.extend_from_slice(summary);
        let style = text::Look {
            size: self.look.size,
            line_height: LINE_HEIGHT,
            weight: BOLD,
            color: self.look.text,
            ..text::Look::PLAIN
        };
        let ink = self.ink(width);
        let paragraph = text::lay_out(self.fonts, &runs(&content, self.look, ink), &style, width);
        self.y += self.text(paragraph, x);
        self.margin = Some(self.look.size);
    }
}
