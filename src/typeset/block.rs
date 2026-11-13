//! Which block of the document each laid-out line comes from.
//!
//! The table has every block that is laid out, a block before the blocks
//! inside it. A line knows two of them: `Line::block`, the top-level block or
//! `<details>` it is in, and `Line::inner`, the innermost block it shows.

use crate::typeset::style::Line;

/// The block a laid-out line comes from: a number in the [`Blocks`] table
/// of the layout it belongs to. It means nothing in another layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockId(u32);

/// What is known of one block. Blocks are ordered as they come in the
/// shown documents: by file, then by path, so a block comes before the
/// blocks inside it and after those above it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockInfo {
    file: usize,
    /// The place of the block in its document: its number among the
    /// top-level blocks, then, for a block inside a `<details>`, its number
    /// among that block's own, and so on. The header of a file has none.
    /// Paths are the ones [`SourceMap`](crate::typeset::model::SourceMap)
    /// describes.
    path: Vec<usize>,
}

impl BlockInfo {
    /// The block of file `file` at `path`.
    pub fn new(file: usize, path: Vec<usize>) -> Self {
        Self { file, path }
    }

    /// The number of the file the block is in, counted from 0. A lone
    /// document counts as file 0.
    pub fn file(&self) -> usize {
        self.file
    }

    /// The place of the block in its document, as described for the
    /// field `path`.
    pub fn path(&self) -> &[usize] {
        &self.path
    }

    /// Whether `other` is the block at the same place, of the same file.
    /// Where the source has the block is not part of it: that is in the
    /// document's [`SourceMap`](crate::typeset::model::SourceMap), by file
    /// and path, so a block that moves down the file stays the same block.
    pub fn same_place(&self, other: &BlockInfo) -> bool {
        self.file == other.file && self.path == other.path
    }
}

/// The blocks of a layout, which [`BlockId`]s number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blocks {
    infos: Vec<BlockInfo>,
    /// The blocks the layout does not show, such as hidden front matter,
    /// by number, in order.
    hidden: Vec<BlockId>,
}

impl Blocks {
    /// Adds a block of file `file` at `path` and returns its number.
    pub(crate) fn add(&mut self, file: usize, path: Vec<usize>) -> BlockId {
        let id = BlockId(u32::try_from(self.infos.len()).unwrap_or(u32::MAX));
        self.infos.push(BlockInfo { file, path });
        id
    }

    /// Notes that the texts of the block `id` are not shown.
    pub(crate) fn hide(&mut self, id: BlockId) {
        self.hidden.push(id);
    }

    /// Whether the layout did not show the texts of the block `id`, which
    /// must be one of this table's: front matter that is hidden, or shown
    /// as its source in place of its keys and values. Its texts are the
    /// document's but are on no line.
    pub fn is_hidden(&self, id: BlockId) -> bool {
        self.hidden.contains(&id)
    }

    /// The blocks whose texts the layout did not show, in order.
    pub fn hidden(&self) -> impl Iterator<Item = &BlockInfo> {
        self.hidden.iter().map(|id| self.get(*id))
    }

    /// What is known of the block `id`, which must be one of this table's.
    pub fn get(&self, id: BlockId) -> &BlockInfo {
        &self.infos[id.0 as usize]
    }

    /// The block each of `lines` comes from, or `None` for a line that is
    /// between blocks. `lines` must be laid out with this table.
    pub fn places(&self, lines: &[Line]) -> Vec<Option<BlockInfo>> {
        (lines.iter())
            .map(|line| line.block.map(|id| self.get(id).clone()))
            .collect()
    }

    /// The numbers of the blocks of the table, in its order.
    #[cfg(test)]
    pub(crate) fn ids(&self) -> impl Iterator<Item = BlockId> + use<> {
        (0..self.infos.len()).map(|number| BlockId(number as u32))
    }

    /// The path of the block `id`, as the tests of the layout read it.
    #[cfg(test)]
    pub(crate) fn path(&self, id: BlockId) -> &[usize] {
        &self.get(id).path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_line_is_given_the_block_it_comes_from() {
        let mut blocks = Blocks::default();
        let first = blocks.add(0, vec![0]);
        let second = blocks.add(1, vec![2, 1]);
        let lines = [
            Line {
                block: Some(first),
                ..Line::blank()
            },
            Line::blank(),
            Line {
                block: Some(second),
                ..Line::blank()
            },
        ];
        let places = blocks.places(&lines);
        assert_eq!(places.len(), 3);
        assert_eq!(places[0].as_ref().map(BlockInfo::path), Some(&[0][..]));
        assert_eq!(places[1], None);
        let last = places[2].as_ref().expect("a block");
        assert_eq!((last.file(), last.path()), (1, &[2, 1][..]));
    }
}
