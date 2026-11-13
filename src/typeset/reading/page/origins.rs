//! What of the document a page's texts and pictures show: the bookkeeping
//! of the flow that the map in `places.rs` reads.
//!
//! A flow knows the path of the block it lays out, relative to the block of
//! the page it is in, and the number of the text of that block that its
//! inlines are. Each text it puts on the page is given them, and each block
//! it lays out is given its pixels.

use std::ops::Range;

use crate::typeset::reading::places::ItemAt;
use crate::typeset::style::{Origin, Origins};
use crate::typeset::text::Cursor;

use super::Flow;

/// The origins of `len` bytes that are the first `len` bytes of the text
/// number `text` of the block, as they are.
pub(super) fn exact(text: u32, len: usize) -> Origins {
    let len = u32::try_from(len).unwrap_or(u32::MAX);
    Origins::from_origins([Origin {
        shown: 0..len,
        ..Origin::exact(text, 0..len)
    }])
}

impl Flow<'_> {
    /// What the first `len` bytes of the text laid out next show: the first
    /// bytes of the text of the block, as they are, if there is one.
    pub(super) fn written(&self, len: usize) -> Origins {
        match self.cursor.get().text() {
            Some(text) => exact(text, len),
            None => Origins::default(),
        }
    }

    /// Starts text number `text` of the block that is laid out, or none, for
    /// what is laid out without one.
    pub(super) fn text_is(&self, text: Option<usize>) {
        self.cursor.set(text.map_or_else(Cursor::off, Cursor::new));
    }

    /// What the next text on the page shows of its block, if it shows any.
    pub(super) fn item_at(&self) -> Option<ItemAt> {
        Some(ItemAt {
            block: 0,
            path: self.path.clone(),
            text: self.cursor.get().text()?,
            picture: None,
        })
    }

    /// Says that the items on the page from the number `from` on are a
    /// picture that stands in the place of the bytes `doc` of the text that
    /// is laid out.
    pub(super) fn picture_is(&mut self, from: usize, doc: Range<usize>) {
        let (Some(at), Some(start), Some(end)) = (
            self.item_at(),
            u32::try_from(doc.start).ok(),
            u32::try_from(doc.end).ok(),
        ) else {
            return;
        };
        for item in &mut self.items[from..] {
            item.at = Some(ItemAt {
                picture: Some(start..end),
                ..at.clone()
            });
        }
    }

    /// Runs `lay_out` one step further along the path, in the block or item
    /// numbered `step`.
    pub(super) fn step(&mut self, step: usize, lay_out: impl FnOnce(&mut Self)) {
        self.path.push(step);
        lay_out(self);
        self.path.pop();
    }

    /// Records that the block at the path, now laid out, covers the pixels
    /// from `top` to the current bottom.
    pub(super) fn covers(&mut self, top: f32) {
        self.extents.push((self.path.clone(), top..self.y));
    }

    /// The blocks laid out inside the block just laid out, with their
    /// pixels, to be kept with the page's block; and the items and bands it
    /// made, from the numbers `items` and `bands`, are the block `number`'s.
    pub(super) fn closed(
        &mut self,
        number: usize,
        (items, bands): (usize, usize),
    ) -> Vec<(Vec<usize>, Range<f32>)> {
        for item in &mut self.items[items..] {
            if let Some(at) = &mut item.at {
                at.block = number;
            }
        }
        for item in self.bands[bands..]
            .iter_mut()
            .flat_map(|band| &mut band.items)
        {
            if let Some(at) = &mut item.at {
                at.block = number;
            }
        }
        std::mem::take(&mut self.extents)
    }
}
