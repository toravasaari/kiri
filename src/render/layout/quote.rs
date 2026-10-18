//! Block quotes.

use super::{Context, Layout};
use crate::model::Block;
use crate::render::style::{Line, Span, Style};

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
}
