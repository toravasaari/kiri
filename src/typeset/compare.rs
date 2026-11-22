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

/// Whether `new` is `old` grown at its end, and the same before it, byte
/// for byte: both a whole file shown as text, in the same language, whose
/// text starts with the old one's, or both the table of a CSV or TSV split
/// at the same separator, with as many columns, whose rows' source starts
/// with the old one's. A layout of `old` can then go on into one of `new`
/// from the last stretch of the block. The bytes are compared, not hashes.
pub fn grown(old: &Document, new: &Document) -> bool {
    match (&old.blocks[..], &new.blocks[..]) {
        (
            [
                Block::CodeBlock {
                    language,
                    code: before,
                    whole_file: true,
                },
            ],
            [
                Block::CodeBlock {
                    language: now,
                    code,
                    whole_file: true,
                },
            ],
        ) => language == now && code.starts_with(&**before),
        ([Block::Table(before), ..], [Block::Table(table), ..]) => {
            match (before.source(), table.source()) {
                (Some(rows), Some(now)) => {
                    before.alignments == table.alignments && rows.grew_into(now)
                }
                _ => false,
            }
        }
        _ => false,
    }
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
mod tests;
