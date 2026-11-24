//! A centred wrapper such as `<p align="center">` in the cell layout: its paragraphs, headings and pictures centred in its room.

use crate::typeset::layout::{Context, Layout};
use crate::typeset::model::Block;
use crate::typeset::style::Line;

impl Layout<'_> {
    pub(super) fn centred(&self, blocks: &[Block], width: usize, context: Context) -> Vec<Line> {
        self.blocks(blocks, width, context)
    }
}
