//! A centred wrapper such as `<p align="center">` in reading mode's page: its paragraphs, headings and pictures centred in its room.
//!
//! The wrapper's blocks are laid out in the wrapper's own flow, one at a
//! time with `centre` set for those that hold no blocks: a paragraph, a
//! heading, a picture. Text is then set centred in its width as a table
//! cell is, and a picture is placed in the middle as a formula is.

use crate::typeset::model::Block;
use crate::typeset::reading::page::{Flow, block_margins};
use crate::typeset::reading::text::Align;

impl Flow<'_> {
    /// Lays the blocks of a centred wrapper out at `x`, `width` pixels
    /// wide, `depth` lists deep, one below another: a paragraph, a heading
    /// and a picture centred, and a table, a code block, a list and a
    /// quote where they would be.
    pub(super) fn centred(&mut self, blocks: &[Block], x: f32, width: f32, depth: usize) {
        for (index, block) in blocks.iter().enumerate() {
            self.top(block_margins(block, self.look).0);
            let leaf = matches!(
                block,
                Block::Paragraph(_) | Block::Heading { .. } | Block::CodeBlock { .. }
            );
            self.centre = leaf;
            self.step(index, |flow| flow.block(block, x, width, depth));
            self.centre = false;
        }
    }

    /// How the lines of text are set: centred in a centred wrapper.
    pub(super) fn align(&self) -> Align {
        match self.centre {
            true => Align::Centre,
            false => Align::Left,
        }
    }
}
