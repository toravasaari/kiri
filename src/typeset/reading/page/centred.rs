//! A centred wrapper such as `<p align="center">` in reading mode's page: its paragraphs, headings and pictures centred in its room.
//!
//! The wrapper's blocks are laid out in the wrapper's own flow, one at a
//! time with `centre` set for those that hold no blocks: a paragraph, a
//! heading, a picture. Text is then set centred in its width as a table
//! cell is, and a picture is placed in the middle as a formula is.
//!
//! `Flow::blocks`, the loop that lays blocks out one below another for the
//! page, a quote and a `<details>` as well, is here: it takes whether to
//! centre, and `Block::is_centred_in_wrapper` tells which blocks are.

use std::hash::Hash;

use crate::typeset::model::Block;
use crate::typeset::reading::page::{Flow, Look, block_margins};
use crate::typeset::reading::text::Align;

/// A block that a page lays out, and whether a centred wrapper centres it:
/// such a block is laid out as the one block of a wrapper, without a copy
/// of it in one.
pub trait Laid: Hash {
    fn block(&self) -> &Block;
    fn centred(&self) -> bool;
}

impl Laid for &Block {
    fn block(&self) -> &Block {
        self
    }

    fn centred(&self) -> bool {
        false
    }
}

/// A block of a page, centred or not, as reading mode's pen holds it.
#[derive(Debug, Clone, Copy, Hash)]
pub struct Piece<'a> {
    pub block: &'a Block,
    pub centred: bool,
}

impl Laid for Piece<'_> {
    fn block(&self) -> &Block {
        self.block
    }

    fn centred(&self) -> bool {
        self.centred
    }
}

/// The margins of a centred wrapper: those of its first block above and of
/// its last below, as it adds none of its own. A browser lets the margin
/// of a heading in a `<div>` through it as well.
pub(super) fn margins(blocks: &[Block], look: &Look) -> (f32, f32) {
    match (blocks.first(), blocks.last()) {
        (Some(first), Some(last)) => (block_margins(first, look).0, block_margins(last, look).1),
        _ => (0.0, look.size),
    }
}

impl Flow<'_> {
    /// Lays `blocks` out one below another from the current top, at `x`,
    /// `width` pixels wide, `depth` lists deep. With `centring` it centres
    /// the blocks that a centred wrapper centres.
    pub(super) fn blocks(
        &mut self,
        blocks: &[Block],
        x: f32,
        width: f32,
        depth: usize,
        centring: bool,
    ) {
        for (index, block) in blocks.iter().enumerate() {
            self.top(block_margins(block, self.look).0);
            self.centre = centring && block.is_centred_in_wrapper();
            self.step(index, |flow| flow.block(block, x, width, depth));
        }
    }

    /// Lays the blocks of a centred wrapper out at `x`, `width` pixels
    /// wide, `depth` lists deep, one below another: a paragraph, a heading
    /// and a picture centred, and a table, a code block, a list and a
    /// quote where they would be.
    pub(super) fn centred(&mut self, blocks: &[Block], x: f32, width: f32, depth: usize) {
        self.blocks(blocks, x, width, depth, true);
        self.centre = false;
    }

    /// Lays the page's block `laid` out at its left, `width` pixels wide:
    /// a centred one as the one block of a wrapper, as `block` lays out a
    /// wrapper.
    pub(super) fn laid(&mut self, laid: &impl Laid, width: f32) {
        let block = laid.block();
        if !laid.centred() {
            return self.block(block, 0.0, width, 0);
        }
        let top = self.y;
        self.text_is(None);
        self.centred(std::slice::from_ref(block), 0.0, width, 0);
        self.margin = Some(block_margins(block, self.look).1);
        self.covers(top);
    }

    /// How the lines of text are set: centred in a centred wrapper.
    pub(super) fn align(&self) -> Align {
        match self.centre {
            true => Align::Centre,
            false => Align::Left,
        }
    }
}
