//! Which blocks of one document stayed, changed, were added or were removed
//! in another.
//!
//! The comparison reads two [`Document`]s and nothing else: it knows
//! neither the parser nor a terminal. The pager uses it on a reload, to keep
//! the place in the document and to find the changes that auto-follow
//! moves to.
//!
//! Blocks are told apart by the paths of
//! [`SourceLines`](crate::model::SourceLines), which say how a block is
//! reached from the top of its document: its number among the top-level
//! blocks, then one number into a `<details>`, quote or alert, or two into
//! a list (the item, then the block in the item). The layout's
//! [`BlockInfo`](crate::render::block::BlockInfo) has to follow the same
//! paths wherever it marks blocks, so that a path from here finds a block
//! there.
//!
//! The lines a document keeps of where its blocks were written take no
//! part, nor does the line a code block keeps for its messages: a block
//! that only moved down the file has stayed.

use std::collections::{BTreeMap, BTreeSet};
use std::mem;

use crate::model::{Block, Document, ListItem};

/// A place in a document, as described above.
pub type Path = Vec<usize>;

/// What became of a block of the new document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fate {
    /// The same block as the old document's block at `was`, with the same
    /// content.
    Stayed { was: Path },
    /// The block of the old document at `was`, with other content. It is
    /// the block of the same kind that stood in its place.
    Changed { was: Path },
    /// A block the old document does not have.
    Added,
}

/// How a new document differs from an old one, block by block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comparison {
    new: BTreeMap<Path, Fate>,
    removed: BTreeSet<Path>,
}

impl Comparison {
    /// What became of the block at `path` of the new document, or `None`
    /// where it has no such block.
    pub fn fate(&self, path: &[usize]) -> Option<&Fate> {
        self.new.get(path)
    }

    /// What became of each block of the new document, by its path, in
    /// order.
    pub fn fates(&self) -> impl Iterator<Item = (&Path, &Fate)> {
        self.new.iter()
    }

    /// The paths in the old document of the blocks that are gone, in order.
    pub fn removed(&self) -> impl Iterator<Item = &Path> {
        self.removed.iter()
    }

    /// Whether every block stayed and none was added or removed.
    pub fn is_unchanged(&self) -> bool {
        self.removed.is_empty()
            && self
                .new
                .values()
                .all(|fate| matches!(fate, Fate::Stayed { .. }))
    }
}

/// Compares `new` with `old`.
pub fn compare(old: &Document, new: &Document) -> Comparison {
    let (old, new) = (without_lines(&old.blocks), without_lines(&new.blocks));
    let mut walk = Walk::default();
    walk.blocks(&old, &new, &[], &[]);
    walk.comparison
}

/// Every block of `document` with its path, in order, each before the
/// blocks inside it.
pub fn blocks(document: &Document) -> Vec<(Path, &Block)> {
    fn walk<'a>(blocks: &'a [Block], path: &[usize], out: &mut Vec<(Path, &'a Block)>) {
        for (index, block) in blocks.iter().enumerate() {
            let path = join(path, index);
            out.push((path.clone(), block));
            match block {
                Block::BlockQuote(blocks)
                | Block::Alert { blocks, .. }
                | Block::Details { blocks, .. } => walk(blocks, &path, out),
                Block::List(list) => {
                    for (item, list_item) in list.items.iter().enumerate() {
                        walk(&list_item.blocks, &join(&path, item), out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&document.blocks, &[], &mut out);
    out
}

/// `blocks` with the line of every code block in them set to 0, so that a
/// code block that only moved is equal to where it was.
fn without_lines(blocks: &[Block]) -> Vec<Block> {
    let mut blocks = blocks.to_vec();
    clear_lines(&mut blocks);
    blocks
}

fn clear_lines(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::CodeBlock { line, .. } => *line = 0,
            Block::BlockQuote(blocks)
            | Block::Alert { blocks, .. }
            | Block::Details { blocks, .. } => clear_lines(blocks),
            Block::List(list) => {
                for item in &mut list.items {
                    clear_lines(&mut item.blocks);
                }
            }
            _ => {}
        }
    }
}

/// The largest table of the alignment of two lists of siblings, in cells.
/// Past it, siblings that do not stay as they are are only paired by kind,
/// which is cheap and still finds every block. A block added there makes
/// the later siblings of its kind changed rather than stayed.
const MAX_CELLS: usize = 4_000_000;

#[derive(Default)]
struct Walk {
    comparison: Comparison,
}

/// What became of one of two lists of siblings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The old sibling at the first index and the new one at the second.
    Pair(usize, usize),
    Removed(usize),
    Added(usize),
}

impl Walk {
    /// Compares the siblings `old` at `old_path` with `new` at `new_path`.
    fn blocks(&mut self, old: &[Block], new: &[Block], old_path: &[usize], new_path: &[usize]) {
        for step in align(old, new, |a, b| {
            mem::discriminant(a) == mem::discriminant(b)
        }) {
            match step {
                Step::Pair(i, j) => {
                    let (was, now) = (join(old_path, i), join(new_path, j));
                    let fate = if old[i] == new[j] {
                        Fate::Stayed { was: was.clone() }
                    } else {
                        Fate::Changed { was: was.clone() }
                    };
                    self.comparison.new.insert(now.clone(), fate);
                    self.inside(Some(&old[i]), Some(&new[j]), &was, &now);
                }
                Step::Removed(i) => {
                    let was = join(old_path, i);
                    self.comparison.removed.insert(was.clone());
                    self.inside(Some(&old[i]), None, &was, &[]);
                }
                Step::Added(j) => {
                    let now = join(new_path, j);
                    self.comparison.new.insert(now.clone(), Fate::Added);
                    self.inside(None, Some(&new[j]), &[], &now);
                }
            }
        }
    }

    /// Compares what is inside the blocks `old` and `new`, which are of one
    /// kind, or where one of them is missing, with nothing.
    fn inside(
        &mut self,
        old: Option<&Block>,
        new: Option<&Block>,
        old_path: &[usize],
        new_path: &[usize],
    ) {
        fn blocks(block: Option<&Block>) -> &[Block] {
            match block {
                Some(Block::BlockQuote(blocks))
                | Some(Block::Alert { blocks, .. })
                | Some(Block::Details { blocks, .. }) => blocks,
                _ => &[],
            }
        }
        fn items(block: Option<&Block>) -> &[ListItem] {
            match block {
                Some(Block::List(list)) => &list.items,
                _ => &[],
            }
        }
        self.blocks(blocks(old), blocks(new), old_path, new_path);
        self.items(items(old), items(new), old_path, new_path);
    }

    /// Compares list items, which are all of one kind: the ones that stay
    /// as they are are found first, and the others pair up in order.
    fn items(
        &mut self,
        old: &[ListItem],
        new: &[ListItem],
        old_path: &[usize],
        new_path: &[usize],
    ) {
        for step in align(old, new, |_, _| true) {
            match step {
                Step::Pair(i, j) => {
                    let (was, now) = (join(old_path, i), join(new_path, j));
                    self.blocks(&old[i].blocks, &new[j].blocks, &was, &now);
                }
                Step::Removed(i) => self.blocks(&old[i].blocks, &[], &join(old_path, i), &[]),
                Step::Added(j) => self.blocks(&[], &new[j].blocks, &[], &join(new_path, j)),
            }
        }
    }
}

/// `path` and then `index`.
fn join(path: &[usize], index: usize) -> Path {
    let mut joined = path.to_vec();
    joined.push(index);
    joined
}

/// Lines up `old` with `new`: the longest run of siblings in the same order
/// that are equal stay as they are, and in each gap between them, old
/// siblings are paired in order with the next new one for which
/// `same_kind` holds. The rest are removed or added.
fn align<T: PartialEq>(old: &[T], new: &[T], same_kind: impl Fn(&T, &T) -> bool) -> Vec<Step> {
    // Siblings that are equal at either end need no searching.
    let start = (old.iter().zip(new)).take_while(|(a, b)| a == b).count();
    let end = (old[start..].iter().rev().zip(new[start..].iter().rev()))
        .take_while(|(a, b)| a == b)
        .count();
    let (old_end, new_end) = (old.len() - end, new.len() - end);
    let matches = longest_common(&old[start..old_end], &new[start..new_end]);

    let mut steps: Vec<Step> = (0..start).map(|i| Step::Pair(i, i)).collect();
    let (mut i, mut j) = (start, start);
    for (a, b) in matches
        .into_iter()
        .map(|(a, b)| (a + start, b + start))
        .chain([(old_end, new_end)])
    {
        gap(&old[i..a], &new[j..b], i, j, &same_kind, &mut steps);
        if a < old_end {
            steps.push(Step::Pair(a, b));
        }
        (i, j) = (a + 1, b + 1);
    }
    steps.extend((0..end).map(|k| Step::Pair(old_end + k, new_end + k)));
    steps
}

/// Pairs the siblings `old` and `new` of a gap, which start at the indexes
/// `i` and `j` of their lists, and adds the steps to `steps`.
fn gap<T>(
    old: &[T],
    new: &[T],
    i: usize,
    j: usize,
    same_kind: impl Fn(&T, &T) -> bool,
    steps: &mut Vec<Step>,
) {
    let mut taken = vec![false; new.len()];
    let mut next = 0;
    for (a, sibling) in old.iter().enumerate() {
        match (next..new.len()).find(|&b| same_kind(sibling, &new[b])) {
            Some(b) => {
                taken[b] = true;
                next = b + 1;
                steps.push(Step::Pair(i + a, j + b));
            }
            None => steps.push(Step::Removed(i + a)),
        }
    }
    steps.extend(
        (0..new.len())
            .filter(|&b| !taken[b])
            .map(|b| Step::Added(j + b)),
    );
}

/// The index pairs of a longest run of equal siblings of `old` and `new`
/// in the same order, or none when the lists are too long to search.
fn longest_common<T: PartialEq>(old: &[T], new: &[T]) -> Vec<(usize, usize)> {
    let (n, m) = (old.len(), new.len());
    if n == 0 || m == 0 || n.saturating_mul(m) > MAX_CELLS {
        return Vec::new();
    }
    // `table[a][b]`: the length of the longest run in `old[a..]` and `new[b..]`.
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for a in (0..n).rev() {
        for b in (0..m).rev() {
            table[a][b] = if old[a] == new[b] {
                table[a + 1][b + 1] + 1
            } else {
                table[a + 1][b].max(table[a][b + 1])
            };
        }
    }
    let (mut a, mut b) = (0, 0);
    let mut pairs = Vec::new();
    while a < n && b < m {
        if old[a] == new[b] {
            pairs.push((a, b));
            (a, b) = (a + 1, b + 1);
        } else if table[a + 1][b] >= table[a][b + 1] {
            a += 1;
        } else {
            b += 1;
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn compared(old: &str, new: &str) -> Comparison {
        compare(&parse(old), &parse(new))
    }

    fn fate(comparison: &Comparison, path: &[usize]) -> Option<Fate> {
        comparison.fate(path).cloned()
    }

    fn stayed(was: &[usize]) -> Option<Fate> {
        Some(Fate::Stayed { was: was.to_vec() })
    }

    fn changed(was: &[usize]) -> Option<Fate> {
        Some(Fate::Changed { was: was.to_vec() })
    }

    fn added() -> Option<Fate> {
        Some(Fate::Added)
    }

    fn removed(comparison: &Comparison) -> Vec<Path> {
        comparison.removed().cloned().collect()
    }

    #[test]
    fn identical_documents_have_every_block_stayed() {
        let source = "# Title\n\n- a\n- b\n\n> quote\n\n```\ncode\n```\n";
        let comparison = compared(source, source);
        assert!(comparison.is_unchanged());
        assert_eq!(fate(&comparison, &[0]), stayed(&[0]));
        assert_eq!(fate(&comparison, &[1, 1, 0]), stayed(&[1, 1, 0]));
        assert_eq!(fate(&comparison, &[2, 0]), stayed(&[2, 0]));
        assert_eq!(fate(&comparison, &[3]), stayed(&[3]));
        assert_eq!(fate(&comparison, &[4]), None);
    }

    #[test]
    fn the_fates_of_the_new_blocks_come_in_order_of_their_paths() {
        let comparison = compared("a\n\n- b\n", "x\n\na\n\n- b\n- c\n");
        let fates: Vec<(Path, Fate)> = comparison
            .fates()
            .map(|(path, fate)| (path.clone(), fate.clone()))
            .collect();
        assert_eq!(
            fates,
            [
                (vec![0], Fate::Added),
                (vec![1], Fate::Stayed { was: vec![0] }),
                (vec![2], Fate::Changed { was: vec![1] }),
                (vec![2, 0, 0], Fate::Stayed { was: vec![1, 0, 0] }),
                (vec![2, 1, 0], Fate::Added),
            ]
        );
    }

    #[test]
    fn empty_documents_have_nothing_to_tell() {
        let comparison = compared("", "");
        assert!(comparison.is_unchanged());
        assert_eq!(fate(&comparison, &[0]), None);
        assert_eq!(removed(&comparison), Vec::<Path>::new());
    }

    #[test]
    fn everything_is_added_to_an_empty_document() {
        let comparison = compared("", "one\n\n> two\n");
        assert_eq!(fate(&comparison, &[0]), added());
        assert_eq!(fate(&comparison, &[1]), added());
        assert_eq!(fate(&comparison, &[1, 0]), added());
        assert_eq!(removed(&comparison), Vec::<Path>::new());
        assert!(!comparison.is_unchanged());
    }

    #[test]
    fn everything_is_removed_from_a_document_that_ends_up_empty() {
        let comparison = compared("one\n\n> two\n", "");
        assert_eq!(removed(&comparison), [vec![0], vec![1], vec![1, 0]]);
        assert!(!comparison.is_unchanged());
    }

    #[test]
    fn top_level_blocks_stay_change_and_are_added() {
        let comparison = compared(
            "# Title\n\nkeep\n\nbefore\n\n---\n",
            "# Title\n\nkeep\n\nafter\n\n---\n\n```\ncode\n```\n",
        );
        assert_eq!(fate(&comparison, &[0]), stayed(&[0]));
        assert_eq!(fate(&comparison, &[1]), stayed(&[1]));
        assert_eq!(fate(&comparison, &[2]), changed(&[2]));
        assert_eq!(fate(&comparison, &[3]), stayed(&[3]));
        assert_eq!(fate(&comparison, &[4]), added());
        assert_eq!(removed(&comparison), Vec::<Path>::new());
    }

    #[test]
    fn a_removed_block_is_found_at_its_old_path() {
        let comparison = compared("one\n\n# Head\n\ntwo\n", "one\n\ntwo\n");
        assert_eq!(removed(&comparison), [vec![1]]);
        assert_eq!(fate(&comparison, &[0]), stayed(&[0]));
        // A block that stayed has the path it had.
        assert_eq!(fate(&comparison, &[1]), stayed(&[2]));
    }

    #[test]
    fn a_block_of_another_kind_is_removed_and_another_added() {
        let comparison = compared("text\n", "# text\n");
        assert_eq!(removed(&comparison), [vec![0]]);
        assert_eq!(fate(&comparison, &[0]), added());
    }

    #[test]
    fn blocks_of_one_kind_in_a_gap_pair_up_in_order() {
        let comparison = compared("keep\n\none\n\ntwo\n\nend\n", "keep\n\nthree\n\nend\n");
        assert_eq!(fate(&comparison, &[1]), changed(&[1]));
        assert_eq!(removed(&comparison), [vec![2]]);
        assert_eq!(fate(&comparison, &[2]), stayed(&[3]));
    }

    #[test]
    fn the_lines_blocks_were_written_on_do_not_count() {
        let comparison = compared("a\n\nb\n", "a\n\n\n\n\nb\n");
        assert!(comparison.is_unchanged());
    }

    #[test]
    fn a_code_block_that_only_moved_down_has_stayed() {
        let comparison = compared("a\n\n```\nx\n```\n", "a\n\nnew\n\n```\nx\n```\n");
        assert_eq!(fate(&comparison, &[2]), stayed(&[1]));
        let comparison = compared("> ```\n> x\n> ```\n", "\n\n> ```\n> x\n> ```\n");
        assert!(comparison.is_unchanged());
    }

    #[test]
    fn front_matter_is_the_first_block() {
        let comparison = compared("---\na: 1\n---\n# H\n", "---\na: 2\n---\n# H\n");
        assert_eq!(fate(&comparison, &[0]), changed(&[0]));
        assert_eq!(fate(&comparison, &[1]), stayed(&[1]));
    }

    #[test]
    fn the_blocks_of_a_details_that_changed_are_compared() {
        let old = "<details>\n<summary>S</summary>\n\nfirst\n\nsecond\n\n</details>\n";
        let new = "<details>\n<summary>S</summary>\n\nfirst\n\nsecond!\n\nthird\n\n</details>\n";
        let comparison = compared(old, new);
        assert_eq!(fate(&comparison, &[0]), changed(&[0]));
        assert_eq!(fate(&comparison, &[0, 0]), stayed(&[0, 0]));
        assert_eq!(fate(&comparison, &[0, 1]), changed(&[0, 1]));
        assert_eq!(fate(&comparison, &[0, 2]), added());
    }

    #[test]
    fn a_details_with_a_new_summary_has_its_blocks_stay() {
        let old = "<details>\n<summary>Old</summary>\n\nbody\n\n</details>\n";
        let new = "<details>\n<summary>New</summary>\n\nbody\n\n</details>\n";
        let comparison = compared(old, new);
        assert_eq!(fate(&comparison, &[0]), changed(&[0]));
        assert_eq!(fate(&comparison, &[0, 0]), stayed(&[0, 0]));
    }

    #[test]
    fn list_items_are_compared_item_by_item() {
        let comparison = compared("- x\n- y\n", "- x\n- new\n- y\n");
        assert_eq!(fate(&comparison, &[0]), changed(&[0]));
        assert_eq!(fate(&comparison, &[0, 0, 0]), stayed(&[0, 0, 0]));
        assert_eq!(fate(&comparison, &[0, 1, 0]), added());
        assert_eq!(fate(&comparison, &[0, 2, 0]), stayed(&[0, 1, 0]));
        assert_eq!(removed(&comparison), Vec::<Path>::new());
    }

    #[test]
    fn the_blocks_of_a_list_item_are_compared() {
        let comparison = compared("- x\n\n  one\n- y\n", "- x\n\n  two\n- y\n");
        assert_eq!(fate(&comparison, &[0]), changed(&[0]));
        assert_eq!(fate(&comparison, &[0, 0, 0]), stayed(&[0, 0, 0]));
        assert_eq!(fate(&comparison, &[0, 0, 1]), changed(&[0, 0, 1]));
        assert_eq!(fate(&comparison, &[0, 1, 0]), stayed(&[0, 1, 0]));
    }

    #[test]
    fn a_list_item_that_is_gone_has_its_blocks_removed() {
        let comparison = compared("- x\n- y\n", "- x\n");
        assert_eq!(removed(&comparison), [vec![0, 1, 0]]);
        assert_eq!(fate(&comparison, &[0]), changed(&[0]));
    }

    #[test]
    fn the_blocks_of_a_quote_and_of_an_alert_are_compared() {
        let comparison = compared("> a\n>\n> b\n", "> a\n>\n> c\n");
        assert_eq!(fate(&comparison, &[0]), changed(&[0]));
        assert_eq!(fate(&comparison, &[0, 0]), stayed(&[0, 0]));
        assert_eq!(fate(&comparison, &[0, 1]), changed(&[0, 1]));
        let comparison = compared("> [!NOTE]\n> a\n", "> [!NOTE]\n> a\n>\n> b\n");
        assert_eq!(fate(&comparison, &[0]), changed(&[0]));
        assert_eq!(fate(&comparison, &[0, 0]), stayed(&[0, 0]));
        assert_eq!(fate(&comparison, &[0, 1]), added());
    }

    #[test]
    fn a_removed_container_has_the_blocks_in_it_removed() {
        let comparison = compared("keep\n\n> a\n>\n> - b\n", "keep\n");
        assert_eq!(
            removed(&comparison),
            [vec![1], vec![1, 0], vec![1, 1], vec![1, 1, 0, 0]]
        );
    }

    #[test]
    fn an_added_container_has_the_blocks_in_it_added() {
        let comparison = compared("keep\n", "keep\n\n- a\n- b\n");
        assert_eq!(fate(&comparison, &[1]), added());
        assert_eq!(fate(&comparison, &[1, 0, 0]), added());
        assert_eq!(fate(&comparison, &[1, 1, 0]), added());
    }

    #[test]
    fn huge_documents_are_still_compared() {
        let make = |word: &str| {
            (0..3_000)
                .map(|n| format!("{word} {n}\n\n"))
                .collect::<String>()
        };
        let comparison = compared(&make("old"), &make("new"));
        assert_eq!(fate(&comparison, &[0]), changed(&[0]));
        assert_eq!(fate(&comparison, &[2_999]), changed(&[2_999]));
        assert_eq!(removed(&comparison), Vec::<Path>::new());
        // An equal block among them, not at an end, still stays.
        let around = |word: &str| make(word) + "same\n\n" + &make(word);
        let comparison = compared(&around("old"), &around("new"));
        assert_eq!(fate(&comparison, &[3_000]), stayed(&[3_000]));
    }

    #[test]
    fn every_block_has_the_path_the_comparison_tells_it_by() {
        let document = crate::parse::parse("A\n\n> B\n\n- C\n\n  D\n- E\n");
        let new = crate::parse::parse("A\n\n> B\n\n- C\n\n  D!\n- E\n");
        let paths: Vec<Path> = blocks(&document)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert_eq!(
            paths,
            [
                vec![0],
                vec![1],
                vec![1, 0],
                vec![2],
                vec![2, 0, 0],
                vec![2, 0, 1],
                vec![2, 1, 0],
            ]
        );
        let changes = compare(&document, &new);
        for (path, _) in blocks(&new) {
            assert!(changes.fate(&path).is_some(), "{path:?}");
        }
        assert_eq!(
            changes.fate(&[2, 0, 1]),
            Some(&Fate::Changed { was: vec![2, 0, 1] })
        );
    }
}
