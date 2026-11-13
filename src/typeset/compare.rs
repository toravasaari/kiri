//! Which blocks of one document stayed, changed, were added or were removed
//! in another.
//!
//! The comparison reads two [`Document`]s and nothing else: it knows
//! neither the parser nor a terminal. The pager uses it on a reload, to keep
//! the place in the document and to find the changes that auto-follow
//! moves to.
//!
//! Blocks are told apart by the paths of
//! [`SourceMap`](crate::typeset::model::SourceMap), which the model
//! describes: how a block is reached from the top of its document. The
//! layout's [`BlockInfo`](crate::typeset::block::BlockInfo) has to follow the
//! same paths wherever it marks blocks, so that a path from here finds a
//! block there.
//!
//! The lines a document keeps of where its blocks were written take no
//! part: a block that only moved down the file has stayed.
//!
//! Blocks are compared by their hashes ([`Hashes`]), which whoever holds a
//! version of a document keeps beside it. A block with the hash it had has
//! stayed with everything inside it, so the comparison does not go inside
//! it, and [`Comparison::fate`] works out what became of a block inside one
//! that stayed.

use std::collections::{BTreeMap, BTreeSet};
use std::mem;

use crate::typeset::model::{Block, Contents, Document, Hashes, ListItem};

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
///
/// A block that stayed is told of once: the blocks inside it stayed too, at
/// the same paths below it, and [`fate`](Comparison::fate) answers for them
/// from it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comparison {
    new: BTreeMap<Path, Fate>,
    /// The items of lists that stayed as they were in a list that did not:
    /// the path of the item in the new document, and in the old.
    kept: BTreeMap<Path, Path>,
    removed: BTreeSet<Path>,
}

impl Comparison {
    /// What became of the block at `path` of the new document, or `None`
    /// where it has no such block. A block inside one that stayed is told
    /// from that one, so a path that no block of the new document has, but
    /// that leads inside a block that stayed, is told as stayed too.
    pub fn fate(&self, path: &[usize]) -> Option<Fate> {
        if let Some(fate) = self.new.get(path) {
            return Some(fate.clone());
        }
        for at in (1..path.len()).rev() {
            let (head, rest) = path.split_at(at);
            let was = match self.new.get(head) {
                Some(Fate::Stayed { was }) => was,
                Some(Fate::Changed { .. } | Fate::Added) => return None,
                None => match self.kept.get(head) {
                    Some(was) => was,
                    None => continue,
                },
            };
            return Some(Fate::Stayed {
                was: [was.as_slice(), rest].concat(),
            });
        }
        None
    }

    /// What became of each block of the new document that is not inside
    /// one that stayed, by its path, in order. Whatever is inside a block
    /// that stayed has stayed too: ask [`fate`](Comparison::fate).
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
    hashed(old, &old.hashes(), new, &new.hashes())
}

/// Compares `new` with `old` by the hashes of their blocks, which whoever
/// holds a version has kept beside it ([`Document::hashes`]): what has the
/// same hash is taken to be the same block and is not looked into. A hash is
/// 64 bits, so two blocks that differ may have the same one; the odds are
/// negligible, and the harm would be a changed block that is not marked as
/// changed.
pub fn hashed(
    old: &Document,
    old_hashes: &Hashes,
    new: &Document,
    new_hashes: &Hashes,
) -> Comparison {
    let mut walk = Walk {
        old: old_hashes,
        new: new_hashes,
        comparison: Comparison::default(),
    };
    walk.blocks(&old.blocks, &new.blocks, &[], &[]);
    walk.comparison
}

/// Every block of `document` with its path, in order, each before the
/// blocks inside it.
pub fn blocks(document: &Document) -> Vec<(Path, &Block)> {
    document.blocks()
}

/// The largest table of the alignment of two lists of siblings, in cells.
/// Past it, siblings that do not stay as they are are only paired by kind,
/// which is cheap and still finds every block. A block added there makes
/// the later siblings of its kind changed rather than stayed.
const MAX_CELLS: usize = 4_000_000;

struct Walk<'a> {
    old: &'a Hashes,
    new: &'a Hashes,
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

impl Walk<'_> {
    /// Compares the siblings `old` at `old_path` with `new` at `new_path`.
    fn blocks(&mut self, old: &[Block], new: &[Block], old_path: &[usize], new_path: &[usize]) {
        let hashes = |hashes: &Hashes, path: &[usize], len: usize| -> Vec<u64> {
            (0..len)
                .map(|i| hashes.block(&join(path, i)).unwrap_or_default())
                .collect()
        };
        let (old_hashes, new_hashes) = (
            hashes(self.old, old_path, old.len()),
            hashes(self.new, new_path, new.len()),
        );
        let same_kind =
            |i: usize, j: usize| mem::discriminant(&old[i]) == mem::discriminant(&new[j]);
        for step in align(&old_hashes, &new_hashes, same_kind) {
            match step {
                Step::Pair(i, j) => {
                    let (was, now) = (join(old_path, i), join(new_path, j));
                    if old_hashes[i] == new_hashes[j] {
                        // Nothing inside it changed either.
                        self.comparison.new.insert(now, Fate::Stayed { was });
                        continue;
                    }
                    self.comparison
                        .new
                        .insert(now.clone(), Fate::Changed { was: was.clone() });
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
        fn contents(block: Option<&Block>) -> Contents<'_> {
            block.map_or(Contents::None, Block::contents)
        }
        fn blocks(contents: Contents<'_>) -> &[Block] {
            match contents {
                Contents::Blocks(blocks) => blocks,
                Contents::None | Contents::Items(_) => &[],
            }
        }
        fn items(contents: Contents<'_>) -> &[ListItem] {
            match contents {
                Contents::Items(items) => items,
                Contents::None | Contents::Blocks(_) => &[],
            }
        }
        let (old, new) = (contents(old), contents(new));
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
        let hashes = |hashes: &Hashes, path: &[usize], len: usize| -> Vec<u64> {
            (0..len)
                .map(|i| hashes.item(path, i).unwrap_or_default())
                .collect()
        };
        let (old_hashes, new_hashes) = (
            hashes(self.old, old_path, old.len()),
            hashes(self.new, new_path, new.len()),
        );
        for step in align(&old_hashes, &new_hashes, |_, _| true) {
            match step {
                Step::Pair(i, j) => {
                    let (was, now) = (join(old_path, i), join(new_path, j));
                    if old_hashes[i] == new_hashes[j] {
                        self.comparison.kept.insert(now, was);
                    } else {
                        self.blocks(&old[i].blocks, &new[j].blocks, &was, &now);
                    }
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

/// Lines up the siblings with hashes `old` with those with hashes `new`: the
/// longest run of siblings in the same order that have equal hashes stay as
/// they are, and in each gap between them, old siblings are paired in order
/// with the next new one for which `same_kind` holds. The rest are removed or
/// added. `same_kind` takes the index of an old and of a new sibling.
fn align(old: &[u64], new: &[u64], same_kind: impl Fn(usize, usize) -> bool) -> Vec<Step> {
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
        gap((i, a), (j, b), &same_kind, &mut steps);
        if a < old_end {
            steps.push(Step::Pair(a, b));
        }
        (i, j) = (a + 1, b + 1);
    }
    steps.extend((0..end).map(|k| Step::Pair(old_end + k, new_end + k)));
    steps
}

/// Pairs the old siblings `old.0..old.1` and the new ones `new.0..new.1` of
/// a gap, and adds the steps to `steps`.
fn gap(
    old: (usize, usize),
    new: (usize, usize),
    same_kind: impl Fn(usize, usize) -> bool,
    steps: &mut Vec<Step>,
) {
    let mut taken = vec![false; new.1 - new.0];
    let mut next = new.0;
    for a in old.0..old.1 {
        match (next..new.1).find(|&b| same_kind(a, b)) {
            Some(b) => {
                taken[b - new.0] = true;
                next = b + 1;
                steps.push(Step::Pair(a, b));
            }
            None => steps.push(Step::Removed(a)),
        }
    }
    steps.extend(
        (new.0..new.1)
            .filter(|&b| !taken[b - new.0])
            .map(Step::Added),
    );
}

/// The index pairs of a longest run of equal hashes of `old` and `new` in
/// the same order, or none when the lists are too long to search.
fn longest_common(old: &[u64], new: &[u64]) -> Vec<(usize, usize)> {
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

/// A document with blocks nested in every kind of container, which the tests
/// of the path rule here and in the layout share: a list in a quote inside a
/// `<details>`, an alert in a list, and a list in a list.
#[cfg(test)]
pub(crate) const NESTED: &str = "Intro\n\n<details>\n<summary>Sum</summary>\n\n\
    > - first\n>\n>   > [!NOTE]\n>   > deep\n>\n> - second\n\n</details>\n\n\
    - outer\n\n  > [!TIP]\n  > in a list\n\n  - inner\n\n- next\n\nTail\n";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::parse::parse;

    fn compared(old: &str, new: &str) -> Comparison {
        compare(&parse(old), &parse(new))
    }

    fn fate(comparison: &Comparison, path: &[usize]) -> Option<Fate> {
        comparison.fate(path)
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

    #[test]
    fn blocks_have_the_paths_of_the_path_rule_in_order() {
        let document = parse(NESTED);
        let found: Vec<(Path, &str)> = blocks(&document)
            .into_iter()
            .map(|(path, block)| {
                let kind = match block {
                    Block::Paragraph(_) => "paragraph",
                    Block::Details { .. } => "details",
                    Block::BlockQuote(_) => "quote",
                    Block::Alert { .. } => "alert",
                    Block::List(_) => "list",
                    other => panic!("{other:?}"),
                };
                (path, kind)
            })
            .collect();
        let expected: Vec<(Path, &str)> = vec![
            (vec![0], "paragraph"),
            (vec![1], "details"),
            (vec![1, 0], "quote"),
            (vec![1, 0, 0], "list"),
            (vec![1, 0, 0, 0, 0], "paragraph"),
            (vec![1, 0, 0, 0, 1], "alert"),
            (vec![1, 0, 0, 0, 1, 0], "paragraph"),
            (vec![1, 0, 0, 1, 0], "paragraph"),
            (vec![2], "list"),
            (vec![2, 0, 0], "paragraph"),
            (vec![2, 0, 1], "alert"),
            (vec![2, 0, 1, 0], "paragraph"),
            (vec![2, 0, 2], "list"),
            (vec![2, 0, 2, 0, 0], "paragraph"),
            (vec![2, 1, 0], "paragraph"),
            (vec![3], "paragraph"),
        ];
        assert_eq!(found, expected);
    }

    #[test]
    fn every_source_line_is_at_the_path_of_a_block() {
        let document = parse(NESTED);
        let paths: Vec<Path> = blocks(&document)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        let keys: Vec<&Path> = document.places.paths().collect();
        assert!(!keys.is_empty());
        for key in keys {
            assert!(paths.contains(key), "{key:?} in {paths:?}");
        }
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
                (vec![2, 1, 0], Fate::Added),
            ]
        );
        // The block of the item that stayed is not listed, but it stayed.
        assert_eq!(fate(&comparison, &[2, 0, 0]), stayed(&[1, 0, 0]));
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
        let document = crate::typeset::parse::parse("A\n\n> B\n\n- C\n\n  D\n- E\n");
        let new = crate::typeset::parse::parse("A\n\n> B\n\n- C\n\n  D!\n- E\n");
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
            Some(Fate::Changed { was: vec![2, 0, 1] })
        );
    }

    /// How a version of a document was edited, for the tests that pin what
    /// the comparison says about every path.
    #[derive(Clone, Copy, Debug)]
    enum Edit {
        Same,
        AddedFirst,
        AddedMiddle,
        AddedLast,
        RemovedMiddle,
        WordChanged,
        ListItemAdded,
        AddedInDetails,
        AddedInQuote,
    }

    const EDITS: [Edit; 9] = [
        Edit::Same,
        Edit::AddedFirst,
        Edit::AddedMiddle,
        Edit::AddedLast,
        Edit::RemovedMiddle,
        Edit::WordChanged,
        Edit::ListItemAdded,
        Edit::AddedInDetails,
        Edit::AddedInQuote,
    ];

    fn paragraph(text: &str) -> Block {
        Block::Paragraph(vec![crate::typeset::model::Inline::Text(text.into())])
    }

    /// `document` edited as `edit` says, where it has what the edit needs.
    fn edited(document: &Document, edit: Edit) -> Document {
        let mut new = document.clone();
        let blocks = &mut new.blocks;
        let middle = blocks.len() / 2;
        match edit {
            Edit::Same => {}
            Edit::AddedFirst => blocks.insert(0, paragraph("added")),
            Edit::AddedMiddle => blocks.insert(middle, paragraph("added")),
            Edit::AddedLast => blocks.push(paragraph("added")),
            Edit::RemovedMiddle if !blocks.is_empty() => {
                blocks.remove(middle);
            }
            Edit::WordChanged => {
                if let Some(Block::Paragraph(inlines)) = blocks
                    .iter_mut()
                    .find(|block| matches!(block, Block::Paragraph(_)))
                {
                    inlines.push(crate::typeset::model::Inline::Text(" edited".into()));
                }
            }
            Edit::ListItemAdded => {
                if let Some(Block::List(list)) = blocks
                    .iter_mut()
                    .find(|block| matches!(block, Block::List(_)))
                {
                    list.items.insert(
                        0,
                        ListItem {
                            checked: None,
                            blocks: vec![paragraph("added")],
                        },
                    );
                }
            }
            Edit::AddedInDetails => {
                if let Some(Block::Details { blocks, .. }) = blocks
                    .iter_mut()
                    .find(|block| matches!(block, Block::Details { .. }))
                {
                    blocks.insert(0, paragraph("added"));
                }
            }
            Edit::AddedInQuote => {
                if let Some(Block::BlockQuote(blocks)) = blocks
                    .iter_mut()
                    .find(|block| matches!(block, Block::BlockQuote(_)))
                {
                    blocks.insert(0, paragraph("added"));
                }
            }
            Edit::RemovedMiddle => {}
        }
        new
    }

    fn dotted(path: &[usize]) -> String {
        path.iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(".")
    }

    /// The fate of every path of `new` in one line: `path:S`, `:C` or `:A`
    /// for stayed, changed and added, with the old path after it where it
    /// is not the same, and then `-path` for each block that is gone.
    fn summary(old: &Document, new: &Document) -> String {
        let comparison = compare(old, new);
        let mut parts: Vec<String> = blocks(new)
            .into_iter()
            .map(|(path, _)| {
                let at = dotted(&path);
                match fate(&comparison, &path) {
                    Some(Fate::Stayed { was }) if was == path => format!("{at}:S"),
                    Some(Fate::Stayed { was }) => format!("{at}:S{}", dotted(&was)),
                    Some(Fate::Changed { was }) if was == path => format!("{at}:C"),
                    Some(Fate::Changed { was }) => format!("{at}:C{}", dotted(&was)),
                    Some(Fate::Added) => format!("{at}:A"),
                    None => format!("{at}:?"),
                }
            })
            .collect();
        parts.extend(
            removed(&comparison)
                .iter()
                .map(|path| format!("-{}", dotted(path))),
        );
        parts.join(" ")
    }

    #[test]
    fn the_fate_of_every_path_of_every_document_and_edit_is_pinned() {
        let mut lines = Vec::new();
        // The fixtures only: a sentence added to an example is no reason for
        // this to change.
        let fixtures = crate::testing::documents()
            .into_iter()
            .filter(|path| path.components().any(|part| part.as_os_str() == "fixtures"));
        for path in fixtures {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let old = crate::typeset::parse::parse(&std::fs::read_to_string(&path).unwrap());
            for edit in EDITS {
                let new = edited(&old, edit);
                lines.push(format!("{name} {edit:?}: {}", summary(&old, &new)));
            }
        }
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn the_fates_do_not_list_the_blocks_inside_a_stayed_block_but_fate_answers_for_them() {
        let old = "Intro\n\n- a\n\n  b\n- c\n\n> q\n\nTail\n";
        let comparison = compared(old, &old.replace("Tail", "Tail!"));
        let listed: Vec<&Path> = comparison.fates().map(|(path, _)| path).collect();
        assert_eq!(listed, [&vec![0], &vec![1], &vec![2], &vec![3]]);
        for path in [[1, 0, 0], [1, 0, 1], [1, 1, 0]] {
            assert_eq!(fate(&comparison, &path), stayed(&path), "{path:?}");
        }
        assert_eq!(fate(&comparison, &[2, 0]), stayed(&[2, 0]));
        assert_eq!(fate(&comparison, &[3]), changed(&[3]));
    }

    #[test]
    fn a_block_inside_a_stayed_block_that_moved_has_the_old_path_it_moved_from() {
        let comparison = compared("- a\n\n  b\n", "new\n\n- a\n\n  b\n");
        assert_eq!(fate(&comparison, &[1]), stayed(&[0]));
        assert_eq!(fate(&comparison, &[1, 0, 1]), stayed(&[0, 0, 1]));
    }

    #[test]
    fn what_has_the_same_hash_is_the_same_block_whatever_it_holds() {
        let (old, new) = (parse("A\n\nB\n"), parse("A\n\nC\n"));
        let hashes = old.hashes();
        let comparison = hashed(&old, &hashes, &new, &hashes);
        assert!(comparison.is_unchanged(), "{comparison:?}");
    }

    #[test]
    fn what_has_another_hash_is_not_the_same_block_whatever_it_holds() {
        let (old, new) = (parse("A\n\nB\n"), parse("A\n\nB\n"));
        let (hashes, others) = (old.hashes(), parse("X\n\nY\n").hashes());
        let comparison = hashed(&old, &hashes, &new, &others);
        assert_eq!(fate(&comparison, &[0]), changed(&[0]));
    }
}
