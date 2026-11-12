//! A place in a document: which file, which block, and where in the block.
//!
//! What a document holds is asked of the model, and a place in it is a
//! `Pos`, never a line number or the address of a page: a line or a page
//! belongs to one layout of the document, and a `Pos` to the document. A
//! stretch of a document is a [`Range<Pos>`], as in `start..end`: it
//! may cross the borders of blocks and of files.

use std::cmp::Ordering;

/// A place in the shown documents, in the order of the documents: first the
/// file, then the block, then the place in the block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Pos {
    /// The number of the file, counted from 0 in the order the files are
    /// shown. A document shown alone is file 0.
    pub file: usize,
    /// The block, by the path that [`SourceMap`](super::model::SourceMap)
    /// describes.
    pub path: Vec<usize>,
    pub within: Within,
}

/// Where in a block a [`Pos`] is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Within {
    /// The block itself, before its texts.
    Block,
    /// The byte `offset` of the text number `text` of the block.
    Text { text: usize, offset: usize },
    /// The end of the block: after its own texts and after every block
    /// inside it. So `Pos::block(..)..Pos::end(..)` is the whole block, a
    /// quote, a list or a `<details>` with what it holds, and the range
    /// of a block without text, such as an image file, still holds the
    /// place of the block itself.
    End,
}

impl Pos {
    /// The block at `path` of file `file`, before its texts.
    pub fn block(file: usize, path: &[usize]) -> Self {
        Self {
            file,
            path: path.to_vec(),
            within: Within::Block,
        }
    }

    /// The byte `offset` of text number `text` of the block at `path`.
    pub fn text(file: usize, path: &[usize], text: usize, offset: usize) -> Self {
        Self {
            file,
            path: path.to_vec(),
            within: Within::Text { text, offset },
        }
    }

    /// The end of the block at `path`, after everything inside it.
    pub fn end(file: usize, path: &[usize]) -> Self {
        Self {
            file,
            path: path.to_vec(),
            within: Within::End,
        }
    }
}

impl Within {
    /// The order among places of one block: the block, its texts in
    /// order, the end.
    fn rank(&self) -> (u8, usize, usize) {
        match *self {
            Self::Block => (0, 0, 0),
            Self::Text { text, offset } => (1, text, offset),
            Self::End => (2, 0, 0),
        }
    }
}

impl Ord for Pos {
    /// The order of the document. A path compares as the blocks come in
    /// the document, each before those inside it. When one block holds the
    /// other, its own start and texts are before everything inside, and its
    /// end after.
    fn cmp(&self, other: &Self) -> Ordering {
        self.file.cmp(&other.file).then_with(|| {
            if self.path == other.path {
                return self.within.rank().cmp(&other.within.rank());
            }
            if other.path.starts_with(&self.path) {
                // `self` holds `other`.
                return match self.within {
                    Within::End => Ordering::Greater,
                    _ => Ordering::Less,
                };
            }
            if self.path.starts_with(&other.path) {
                return match other.within {
                    Within::End => Ordering::Less,
                    _ => Ordering::Greater,
                };
            }
            self.path.cmp(&other.path)
        })
    }
}

impl PartialOrd for Pos {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::model::Document;
    use crate::typeset::parse::parse;

    fn documents() -> Vec<(String, Document)> {
        crate::testing::documents()
            .into_iter()
            .map(|path| {
                let source = std::fs::read_to_string(&path).unwrap();
                (path.display().to_string(), parse(&source))
            })
            .collect()
    }

    /// Every place of a block that a test can name, as the model has them:
    /// the block, a text, the end.
    fn places(file: usize, path: &[usize]) -> Vec<Pos> {
        vec![
            Pos::block(file, path),
            Pos::text(file, path, 0, 0),
            Pos::text(file, path, 0, 3),
            Pos::text(file, path, 1, 0),
            Pos::end(file, path),
        ]
    }

    #[test]
    fn the_paths_of_a_document_grow_as_places_of_the_blocks() {
        let all = documents();
        for (name, document) in &all {
            let blocks: Vec<Pos> = (document.blocks().iter())
                .map(|(path, _)| Pos::block(0, path))
                .collect();
            assert!(blocks.windows(2).all(|pair| pair[0] < pair[1]), "{name}");
        }
        // Two files, the second after all of the first.
        let (first, second) = (&all[0].1, &all[1].1);
        let mut both: Vec<Pos> = (first.blocks().iter())
            .map(|(path, _)| Pos::block(0, path))
            .chain(second.blocks().iter().map(|(path, _)| Pos::block(1, path)))
            .collect();
        assert!(both.windows(2).all(|pair| pair[0] < pair[1]));
        both.reverse();
        both.sort();
        assert_eq!(both[0].file, 0);
        assert_eq!(both.last().unwrap().file, 1);
    }

    #[test]
    fn a_block_comes_before_its_texts_and_its_children_and_its_end_after_theirs() {
        // `[2]` is a list with two items, the first with two blocks; `[3]`
        // follows it.
        let chain = [
            Pos::block(0, &[2]),
            Pos::text(0, &[2], 0, 5),
            Pos::block(0, &[2, 0, 0]),
            Pos::text(0, &[2, 0, 0], 1, 0),
            Pos::end(0, &[2, 0, 0]),
            Pos::block(0, &[2, 0, 1]),
            Pos::end(0, &[2, 0, 1]),
            Pos::block(0, &[2, 1, 0]),
            Pos::end(0, &[2, 1, 0]),
            Pos::end(0, &[2]),
            Pos::block(0, &[3]),
            Pos::end(0, &[3]),
        ];
        for (i, a) in chain.iter().enumerate() {
            for (j, b) in chain.iter().enumerate() {
                assert_eq!(a.cmp(b), i.cmp(&j), "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn the_range_of_a_block_holds_every_place_inside_it_and_none_outside() {
        for (name, document) in documents() {
            let blocks = document.blocks();
            // Too many pairs for a large document, so a spread of blocks.
            let step = (blocks.len() / 40).max(1);
            for (path, _) in blocks.iter().step_by(step) {
                let range = Pos::block(0, path)..Pos::end(0, path);
                for (other, _) in &blocks {
                    let inside = other.starts_with(path);
                    for place in places(0, other) {
                        assert_eq!(
                            range.contains(&place) || (other == path && place == range.end),
                            inside,
                            "{name}: {path:?} {place:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_order_is_total_and_transitive() {
        let mut sample = Vec::new();
        for (_, document) in documents() {
            for (path, _) in document.blocks().iter().take(6) {
                sample.extend(places(0, path));
            }
            if sample.len() > 70 {
                break;
            }
        }
        for a in &sample {
            for b in &sample {
                let forward = a.cmp(b);
                assert_eq!(forward, b.cmp(a).reverse(), "{a:?} {b:?}");
                assert_eq!(forward == Ordering::Equal, a == b, "{a:?} {b:?}");
                for c in &sample {
                    if forward == Ordering::Less && b < c {
                        assert!(a < c, "{a:?} {b:?} {c:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_second_file_comes_after_every_place_of_the_first() {
        let last = Pos::end(0, &[99]);
        assert!(last < Pos::block(1, &[0]));
        assert!(Pos::text(0, &[5, 3, 1], 9, 9) < Pos::block(1, &[0]));
        assert!(Pos::end(1, &[0]) > Pos::end(0, &[7]));
    }
}
