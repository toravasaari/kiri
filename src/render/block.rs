//! Which block of the document each laid-out line comes from.

/// The block a laid-out line comes from: a number in the [`Blocks`] table
/// of the layout it belongs to. It means nothing in another layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockId(u32);

/// What is known of one block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockInfo {
    file: usize,
    /// The place of the block in its document: its number among the
    /// top-level blocks, then, for a block inside a `<details>`, its number
    /// among that block's own, and so on. The header of a file has none.
    path: Vec<usize>,
}

impl BlockInfo {
    /// The number of the file the block is in, counted from 0. A lone
    /// document counts as file 0.
    pub fn file(&self) -> usize {
        self.file
    }
}

/// The blocks of a layout, which [`BlockId`]s number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blocks {
    infos: Vec<BlockInfo>,
}

impl Blocks {
    /// Adds a block of file `file` at `path` and returns its number.
    pub(crate) fn add(&mut self, file: usize, path: Vec<usize>) -> BlockId {
        let id = BlockId(u32::try_from(self.infos.len()).unwrap_or(u32::MAX));
        self.infos.push(BlockInfo { file, path });
        id
    }

    /// What is known of the block `id`, which must be one of this table's.
    pub fn get(&self, id: BlockId) -> &BlockInfo {
        &self.infos[id.0 as usize]
    }

    /// How many blocks the table has.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.infos.len()
    }

    /// The path of the block `id`, as the tests of the layout read it.
    #[cfg(test)]
    pub(crate) fn path(&self, id: BlockId) -> &[usize] {
        &self.get(id).path
    }
}
