//! Where a place of one version of a document is in the next.
//!
//! A place that is kept with its version is of no use once the document is
//! read again: its block may have moved, changed or gone. [`Align`] is the
//! way from the place in the old version to the place in the new one, by
//! what [`Comparison`] says became of each block:
//!
//! - in a block that stayed, the place is the same in the block's new path;
//! - in a block that changed, it is in the block's new path at the same
//!   text and offset when the block still has that text, with the offset
//!   cut to the text's length and to a character, and otherwise at the end
//!   of the block's last text, or at the block itself when it has none. The
//!   rule is that simple on purpose: in a paragraph that was edited the
//!   place stays on or near its line, and a table cell stays the cell with
//!   the same number. Finer care, such as finding the same words, belongs
//!   in this one place if it is ever wanted;
//! - in a block that is gone, it is the first block that is left after it
//!   in the old document, and when there is none, the end of the last one
//!   that is left before it, and failing that, the start of the file.
//!
//! The alignment knows nothing of versions. The state engine keeps a
//! version with each place and asks the alignment only for the files
//! whose version changed.

use std::ops::Range;

use crate::typeset::compare::{Comparison, Fate, Path};
use crate::typeset::position::{Pos, Within};
use crate::typeset::text::DocumentText;

/// How the places of one file move from a version of its document to the
/// next.
pub struct Align<'a> {
    file: usize,
    /// The blocks of the old document that are left, by their paths in it,
    /// in the order of the document: each with its path in the new
    /// document, and whether it changed.
    kept: Vec<(Path, Path, bool)>,
    new: &'a DocumentText,
}

impl<'a> Align<'a> {
    /// The alignment of file `file`, whose new version `new` holds, by what
    /// `comparison` says became of its blocks.
    pub fn new(file: usize, comparison: &Comparison, new: &'a DocumentText) -> Self {
        let mut kept: Vec<(Path, Path, bool)> = new
            .paths(file)
            .filter_map(|path| match comparison.fate(path)? {
                Fate::Stayed { was } => Some((was, path.to_vec(), false)),
                Fate::Changed { was } => Some((was, path.to_vec(), true)),
                Fate::Added => None,
            })
            .collect();
        // The blocks that are left keep their order, so this only makes
        // sure of what the searches below rely on.
        kept.sort_by(|a, b| a.0.cmp(&b.0));
        Self { file, kept, new }
    }

    /// The place in the new version that `pos` is in the old.
    pub fn pos(&self, pos: &Pos) -> Pos {
        if pos.file != self.file {
            return pos.clone();
        }
        let file = self.file;
        match self
            .kept
            .binary_search_by(|(old, ..)| old.as_slice().cmp(&pos.path))
        {
            Ok(index) => {
                let (_, new, changed) = &self.kept[index];
                let within = match changed {
                    true => self.within(new, &pos.within),
                    false => pos.within.clone(),
                };
                Pos {
                    file,
                    path: new.clone(),
                    within,
                }
            }
            // The block is gone, and so is everything inside it, so the
            // next block that is left is not one of its own.
            Err(index) => match (index.checked_sub(1), self.kept.get(index)) {
                (_, Some((_, new, _))) => Pos::block(file, new),
                (Some(before), None) => Pos::end(file, &self.kept[before].1),
                (None, None) => Pos::block(file, &[]),
            },
        }
    }

    /// The range in the new version that `range` is in the old; empty when
    /// its end would come out before its start.
    pub fn range(&self, range: &Range<Pos>) -> Range<Pos> {
        let start = self.pos(&range.start);
        let end = self.pos(&range.end).max(start.clone());
        start..end
    }

    /// The path in the new version of the block at `path` of the old, or
    /// `None` when the block is gone.
    pub fn block(&self, path: &[usize]) -> Option<Path> {
        let index = self
            .kept
            .binary_search_by(|(old, ..)| old.as_slice().cmp(path))
            .ok()?;
        Some(self.kept[index].1.clone())
    }

    /// Where in the block `new`, which changed, the place `within` of the
    /// old block goes.
    fn within(&self, new: &[usize], within: &Within) -> Within {
        let Within::Text { text, offset } = within else {
            return within.clone();
        };
        let texts = self.new.texts(self.file, new);
        match (texts.get(*text), texts.last()) {
            (Some(piece), _) => {
                let mut offset = (*offset).min(piece.text.len());
                while !piece.text.is_char_boundary(offset) {
                    offset -= 1;
                }
                Within::Text {
                    text: *text,
                    offset,
                }
            }
            (None, Some(last)) => Within::Text {
                text: texts.len() - 1,
                offset: last.text.len(),
            },
            (None, None) => Within::Block,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::compare::compare;
    use crate::typeset::parse::parse;

    /// `pos` of the old document `old`, as `Align` puts it in `new`.
    fn moved(old: &str, new: &str, pos: Pos) -> Pos {
        let (old, new) = (parse(old), parse(new));
        let text = DocumentText::new([&new]);
        Align::new(0, &compare(&old, &new), &text).pos(&pos)
    }

    #[test]
    fn a_place_in_a_block_that_stayed_follows_it_down_when_a_block_is_added_above() {
        let at = moved("a\n\nbcd\n", "new\n\na\n\nbcd\n", Pos::text(0, &[1], 0, 2));
        assert_eq!(at, Pos::text(0, &[2], 0, 2));
    }

    #[test]
    fn a_place_inside_a_block_that_stayed_follows_it_in_a_details_a_list_and_a_quote() {
        let new =
            "new\n\n<details>\n<summary>S</summary>\n\n> in\n\n- one\n\n  two\n\n</details>\n";
        let old = new.replacen("new\n\n", "", 1);
        // The quote in the details, the second block of the item.
        let quote = moved(&old, new, Pos::text(0, &[0, 0], 0, 1));
        assert_eq!(quote, Pos::text(0, &[1, 0], 0, 1));
        let item = moved(&old, new, Pos::text(0, &[0, 1, 0, 1], 0, 2));
        assert_eq!(item, Pos::text(0, &[1, 1, 0, 1], 0, 2));
    }

    #[test]
    fn a_place_in_a_list_item_follows_the_item_when_an_item_is_added_above() {
        let at = moved(
            "- a\n- bcd\n",
            "- new\n- a\n- bcd\n",
            Pos::text(0, &[0, 1, 0], 0, 1),
        );
        assert_eq!(at, Pos::text(0, &[0, 2, 0], 0, 1));
    }

    #[test]
    fn a_place_in_a_block_that_changed_keeps_its_text_and_offset_while_they_are_there() {
        let at = moved("abcdef\n", "abcXdef\n", Pos::text(0, &[0], 0, 4));
        assert_eq!(at, Pos::text(0, &[0], 0, 4));
    }

    #[test]
    fn the_offset_in_a_text_that_got_shorter_goes_to_its_end_and_not_in_a_character() {
        let at = moved("abcdef\n", "ab\n", Pos::text(0, &[0], 0, 5));
        assert_eq!(at, Pos::text(0, &[0], 0, 2));
        // The offset 2 is in the middle of the first letter, which has two
        // bytes.
        let at = moved("abcdef\n", "é\n", Pos::text(0, &[0], 0, 3));
        assert_eq!(at, Pos::text(0, &[0], 0, 2));
        let at = moved("abcdef\n", "éx\n", Pos::text(0, &[0], 0, 1));
        assert_eq!(at, Pos::text(0, &[0], 0, 0));
    }

    #[test]
    fn a_text_the_changed_block_no_longer_has_goes_to_the_end_of_its_last_text() {
        let old = "|a|b|\n|-|-|\n|c|d|\n|e|f|\n";
        let new = "|a|b|\n|-|-|\n|c|d|\n";
        // Cell 5 is the `f` of the third row.
        let at = moved(old, new, Pos::text(0, &[0], 5, 1));
        assert_eq!(at, Pos::text(0, &[0], 3, 1));
    }

    #[test]
    fn a_changed_block_without_texts_has_the_block_as_the_place() {
        use crate::typeset::model::{Block, Document};
        let file = |name: &str| Document::new(vec![Block::ImageFile(name.into())]);
        let (old, new) = (file("a.png"), file("b.png"));
        let text = DocumentText::new([&new]);
        let comparison = compare(&old, &new);
        let align = Align::new(0, &comparison, &text);
        assert_eq!(
            align.pos(&Pos::text(0, &[0], 0, 3)),
            Pos::block(0, &[0]),
            "{comparison:?}"
        );
    }

    #[test]
    fn a_table_cell_is_the_same_cell_number_when_a_row_is_added_above() {
        let old = "|a|b|\n|-|-|\n|cd|ef|\n";
        let new = "|a|b|\n|-|-|\n|new|row|\n|cd|ef|\n";
        // Cell 2 is `cd`; it is the cell number 2 still, which is `new`.
        let at = moved(old, new, Pos::text(0, &[0], 2, 1));
        assert_eq!(at, Pos::text(0, &[0], 2, 1));
    }

    #[test]
    fn the_block_and_the_end_of_a_block_that_stayed_or_changed_stay_what_they_are() {
        let (old, new) = ("a\n\nb\n\nc\n", "new\n\na\n\nB\n\nc\n");
        assert_eq!(moved(old, new, Pos::block(0, &[0])), Pos::block(0, &[1]));
        assert_eq!(moved(old, new, Pos::end(0, &[0])), Pos::end(0, &[1]));
        assert_eq!(moved(old, new, Pos::block(0, &[1])), Pos::block(0, &[2]));
        assert_eq!(moved(old, new, Pos::end(0, &[1])), Pos::end(0, &[2]));
    }

    #[test]
    fn a_place_in_a_block_that_is_gone_is_the_block_that_follows_it() {
        let at = moved("a\n\nb\n\nc\n", "a\n\nc\n", Pos::text(0, &[1], 0, 1));
        assert_eq!(at, Pos::block(0, &[1]));
        let at = moved("a\n\nb\n\nc\n", "a\n\nc\n", Pos::end(0, &[1]));
        assert_eq!(at, Pos::block(0, &[1]));
    }

    #[test]
    fn a_place_in_the_last_block_when_it_is_gone_is_the_end_of_the_one_before() {
        let at = moved("a\n\nb\n", "a\n", Pos::text(0, &[1], 0, 1));
        assert_eq!(at, Pos::end(0, &[0]));
    }

    #[test]
    fn a_place_in_a_container_that_is_gone_is_what_follows_the_container_and_not_what_is_in_it() {
        let old = "a\n\n> b\n>\n> c\n\nd\n";
        let new = "a\n\nd\n";
        let at = moved(old, new, Pos::text(0, &[1, 1], 0, 0));
        assert_eq!(at, Pos::block(0, &[1]));
    }

    #[test]
    fn a_place_in_a_document_that_is_gone_whole_is_the_start_of_the_file() {
        let at = moved("a\n\nb\n", "", Pos::text(0, &[1], 0, 1));
        assert_eq!(at, Pos::block(0, &[]));
    }

    #[test]
    fn a_place_in_another_file_is_the_same_place() {
        let (old, new) = (parse("a\n"), parse("new\n\na\n"));
        let text = DocumentText::new([&new]);
        let align = Align::new(0, &compare(&old, &new), &text);
        let at = Pos::text(1, &[0], 0, 1);
        assert_eq!(align.pos(&at), at);
    }

    #[test]
    fn a_range_is_aligned_at_both_ends_and_one_that_would_turn_round_is_empty() {
        let (old, new) = (parse("a\n\nb\n\nc\n"), parse("new\n\na\n\nc\n"));
        let text = DocumentText::new([&new]);
        let align = Align::new(0, &compare(&old, &new), &text);
        let range = Pos::text(0, &[0], 0, 0)..Pos::text(0, &[2], 0, 1);
        assert_eq!(
            align.range(&range),
            Pos::text(0, &[1], 0, 0)..Pos::text(0, &[2], 0, 1)
        );
        // `b` is gone: the range of it starts and ends at `c`.
        let gone = Pos::block(0, &[1])..Pos::end(0, &[1]);
        let range = align.range(&gone);
        assert_eq!(range.start, range.end);
        assert_eq!(range.start, Pos::block(0, &[2]));
    }

    #[test]
    fn a_block_has_its_new_path_and_a_block_that_is_gone_has_none() {
        let (old, new) = (parse("a\n\nb\n\nc\n"), parse("new\n\na\n\nc\n"));
        let text = DocumentText::new([&new]);
        let align = Align::new(0, &compare(&old, &new), &text);
        assert_eq!(align.block(&[0]), Some(vec![1]));
        assert_eq!(align.block(&[1]), None);
        assert_eq!(align.block(&[2]), Some(vec![2]));
    }
}
