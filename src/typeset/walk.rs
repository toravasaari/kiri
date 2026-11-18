//! The walk over the pieces of a file, as a place that can be kept: the
//! driver asks for the piece at a place and is given it with the place of the
//! piece after it, so a walk that stops can go on where it stopped, whatever
//! has happened to the layout since.
//!
//! A piece is a block, the summary of a `<details>` (its blocks are pieces of
//! their own, one step deeper), a file's header, or a stretch of a code block
//! that the pen lays out in parts.

use crate::typeset::driver::{Piece, Shape};
use crate::typeset::model::{Block, Table};
use crate::typeset::position::Pos;
use crate::typeset::stretch::{self, Cut, Stretches};
use crate::typeset::text::{self, Part};

/// Where a walk is: the piece it gives next. It owns what it holds, so it
/// can be kept while the document is not borrowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    /// The file's header is next.
    header: bool,
    /// The path of the next block, or of the one after the blocks that end
    /// here: the walk finds the next piece by it.
    path: Vec<usize>,
    /// Where the next stretch starts, when the block at `path` is laid out in
    /// stretches and some are done.
    cut: Option<Cut>,
}

impl Place {
    /// The place of the block at `path`, which is given whole, not in
    /// stretches.
    pub(crate) fn at_block(path: Vec<usize>) -> Self {
        Self {
            header: false,
            path,
            cut: None,
        }
    }

    /// The start of a file: its header when it has one, then its first block.
    pub(crate) fn start(header: bool) -> Self {
        Self {
            header,
            path: vec![0],
            cut: None,
        }
    }
}

/// The pieces of one file.
#[derive(Clone, Copy)]
pub(crate) struct Walk<'d> {
    /// The name its header shows, when it has one.
    header: Option<&'d str>,
    blocks: &'d [Block],
    /// The source lines of a stretch of a code block.
    size: usize,
}

impl<'d> Walk<'d> {
    pub(crate) fn new(header: Option<&'d str>, blocks: &'d [Block], size: usize) -> Self {
        Self {
            header,
            blocks,
            size,
        }
    }

    /// The block at `path`, as a piece sees it: a `<details>` is its summary.
    pub(crate) fn block(&self, path: &[usize]) -> Option<&'d Block> {
        let (last, parents) = path.split_last()?;
        let mut list = self.blocks;
        for &step in parents {
            match list.get(step)? {
                Block::Details { blocks, .. } => list = blocks,
                _ => return None,
            }
        }
        list.get(*last)
    }

    /// `at` moved on past the blocks that end: where the next piece is, or
    /// `None` when there is none.
    fn settled(&self, at: &Place) -> Option<Place> {
        let mut at = at.clone();
        loop {
            let (last, parents) = at.path.split_last()?;
            let mut list = self.blocks;
            for &step in parents {
                match list.get(step) {
                    Some(Block::Details { blocks, .. }) => list = blocks,
                    _ => return None,
                }
            }
            if *last < list.len() {
                return Some(at);
            }
            at.path.pop();
            *at.path.last_mut()? += 1;
            at.cut = None;
        }
    }

    /// The piece at `at` and the place after it; `None` at the end of the
    /// file. A code block that `cuts` says is laid out in stretches, and has
    /// more than `size` lines, is a piece for each of its stretches.
    pub(crate) fn next(
        &self,
        at: &Place,
        cuts: impl Fn(&Block) -> bool,
    ) -> Option<(Piece<'d>, Place)> {
        if at.header {
            let shape = Shape::Header(self.header?);
            let after = Place {
                header: false,
                ..at.clone()
            };
            return Some((
                Piece {
                    shape,
                    path: Vec::new(),
                },
                after,
            ));
        }
        let here = self.settled(at)?;
        let block = self.block(&here.path)?;
        let following = |path: &[usize], step: Option<usize>| {
            let mut path = path.to_vec();
            match step {
                Some(inside) => path.push(inside),
                None => *path.last_mut().expect("a path has a step") += 1,
            }
            Place {
                header: false,
                path,
                cut: None,
            }
        };
        if let Block::CodeBlock { code, .. } = block {
            let stretches = match here.cut {
                Some(cut) => Some(Stretches::resume(block, code, self.size, cut)),
                None => Stretches::of(block, code, self.size).filter(|_| cuts(block)),
            };
            if let Some(mut stretches) = stretches {
                let stretch = stretches.next()?;
                let after = match stretches.at() {
                    Some(cut) => Place {
                        cut: Some(cut),
                        ..here.clone()
                    },
                    None => following(&here.path, None),
                };
                let shape = Shape::Stretch(stretch);
                return Some((
                    Piece {
                        shape,
                        path: here.path,
                    },
                    after,
                ));
            }
        }
        if let Block::Table(table) = block
            && let Some(found) = self.rows(&here, table, || cuts(block))
        {
            return Some(found);
        }
        let (shape, after) = match block {
            Block::Details { summary, blocks } => (
                Shape::Summary {
                    summary,
                    blocks,
                    number: text::number(block, Part::Summary),
                },
                following(&here.path, Some(0)),
            ),
            _ => (Shape::Block(block), following(&here.path, None)),
        };
        Some((
            Piece {
                shape,
                path: here.path,
            },
            after,
        ))
    }

    /// The stretch of the long table `table` at `here`, and the place after
    /// it, when the table is laid out in stretches: when it is cut there
    /// already, or `cuts` says that the pen cuts it and it has more rows than
    /// a stretch.
    fn rows(
        &self,
        here: &Place,
        table: &'d Table,
        cuts: impl FnOnce() -> bool,
    ) -> Option<(Piece<'d>, Place)> {
        let size = stretch::rows_of(table, self.size);
        let total = table.row_count();
        let from = match here.cut {
            Some(cut) => cut.line,
            None if stretch::is_long(table, self.size) && cuts() => 0,
            None => return None,
        };
        let end = from.saturating_add(size).min(total);
        let after = match end < total {
            true => Place {
                cut: Some(Cut { line: end, byte: 0 }),
                ..here.clone()
            },
            false => {
                let mut path = here.path.clone();
                *path.last_mut()? += 1;
                Place::at_block(path)
            }
        };
        let rows = stretch::Rows {
            table,
            from,
            end,
            first: from == 0,
            last: end == total,
        };
        let piece = Piece {
            shape: Shape::Rows(rows),
            path: here.path.clone(),
        };
        Some((piece, after))
    }

    /// The place in the document of the piece at `at`, in file `file`: where
    /// what the walk has not given yet starts. `None` at the end.
    pub(crate) fn pos(&self, at: &Place, file: usize) -> Option<Pos> {
        if at.header {
            return Some(Pos::block(file, &[]));
        }
        let here = self.settled(at)?;
        let block = self.block(&here.path)?;
        match (here.cut, block, text::number(block, Part::Source)) {
            (Some(cut), Block::Table(table), _) => {
                Some(Pos::text(file, &here.path, table.first_text(cut.line), 0))
            }
            (Some(cut), _, Some(number)) => Some(Pos::text(file, &here.path, number, cut.byte)),
            _ => Some(Pos::block(file, &here.path)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::model::Document;
    use crate::typeset::parse::parse;

    /// What each piece of the walk is, by its kind, its path and, for a
    /// stretch, where it starts.
    fn pieces(walk: &Walk, from: &Place, cuts: bool) -> Vec<String> {
        let mut seen = Vec::new();
        let mut at = from.clone();
        while let Some((piece, next)) = walk.next(&at, |_| cuts) {
            seen.push(match piece.shape {
                Shape::Header(name) => format!("header {name}"),
                Shape::Block(_) => format!("block {:?}", piece.path),
                Shape::Summary { .. } => format!("summary {:?}", piece.path),
                Shape::Stretch(stretch) => {
                    format!("stretch {:?} {}", piece.path, stretch.offset)
                }
                Shape::Rows(rows) => format!("rows {:?} {:?}", piece.path, rows.rows()),
            });
            at = next;
        }
        seen
    }

    fn document() -> Document {
        let code = "a\n".repeat(25);
        let source = format!(
            "One\n\n<details>\n<summary>S</summary>\n\nIn\n\n```rust\n{code}```\n\n\
             <details>\n<summary>Empty</summary>\n\n</details>\n\nBack\n\n</details>\n\n\
             ```rust\n{code}```\n\n- a\n- b\n"
        );
        parse(&source)
    }

    #[test]
    fn the_walk_gives_the_pieces_in_the_order_of_the_document() {
        let document = document();
        let walk = Walk::new(Some("a.md"), &document.blocks, 10);
        let all = pieces(&walk, &Place::start(true), true);
        assert_eq!(
            all,
            [
                "header a.md",
                "block [0]",
                "summary [1]",
                "block [1, 0]",
                "stretch [1, 1] 0",
                "stretch [1, 1] 20",
                "stretch [1, 1] 40",
                "summary [1, 2]",
                "block [1, 3]",
                "stretch [2] 0",
                "stretch [2] 20",
                "stretch [2] 40",
                "block [3]",
            ]
        );
        // A pen that does not cut is given the code blocks whole.
        let whole = pieces(&walk, &Place::start(false), false);
        assert!(whole.contains(&"block [1, 1]".to_owned()), "{whole:?}");
        assert!(whole.contains(&"block [2]".to_owned()), "{whole:?}");
        assert_eq!(whole.len(), 8, "{whole:?}");
    }

    #[test]
    fn a_walk_goes_on_from_any_place_it_gave_as_the_walk_from_the_start_does() {
        let document = document();
        let walk = Walk::new(Some("a.md"), &document.blocks, 10);
        let all = pieces(&walk, &Place::start(true), true);
        let mut at = Place::start(true);
        let mut done = 0;
        while let Some((_, next)) = walk.next(&at, |_| true) {
            done += 1;
            assert_eq!(pieces(&walk, &next, true), all[done..], "after {done}");
            at = next;
        }
        assert_eq!(done, all.len());
        assert!(walk.next(&at, |_| true).is_none());
    }

    #[test]
    fn the_place_of_a_walk_is_where_the_next_piece_is_in_the_document() {
        let document = document();
        let walk = Walk::new(Some("a.md"), &document.blocks, 10);
        let mut at = Place::start(true);
        let mut found = Vec::new();
        while let Some((_, next)) = walk.next(&at, |_| true) {
            found.push(walk.pos(&at, 3).unwrap());
            at = next;
        }
        assert_eq!(walk.pos(&at, 3), None);
        assert_eq!(found[0], Pos::block(3, &[]));
        assert_eq!(found[1], Pos::block(3, &[0]));
        // The second stretch starts after the first's 20 bytes.
        assert_eq!(found[5], Pos::text(3, &[1, 1], 0, 20));
        assert!(found.windows(2).all(|pair| pair[0] < pair[1]), "{found:?}");
    }

    #[test]
    fn an_empty_document_has_its_header_and_nothing_else() {
        let walk = Walk::new(Some("a.md"), &[], 10);
        assert_eq!(pieces(&walk, &Place::start(true), true), ["header a.md"]);
        assert!(pieces(&walk, &Place::start(false), true).is_empty());
    }
}
