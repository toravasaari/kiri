//! A centred wrapper such as `<p align="center">` in reading mode's page: its paragraphs, headings and pictures centred in its room.

use crate::typeset::model::Block;
use crate::typeset::reading::page::Flow;

impl Flow<'_> {
    pub(super) fn centred(&mut self, blocks: &[Block], x: f32, width: f32, depth: usize) {
        self.blocks(blocks, x, width, depth);
    }
}
