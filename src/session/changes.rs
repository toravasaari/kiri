//! Where a reload changed the document, in the lines laid out for it, and
//! which of the changes auto-follow moves the view to.
//!
//! The comparison tells by path which blocks of the new document changed
//! or were added, and which blocks of the old one were removed; the layout
//! tells which block each line comes from. A change is a run of lines
//! whose blocks changed or were added, the blank lines between them
//! included. A removal with no such line beside it is a change of no
//! lines, at the first line of the block that now follows the removed
//! blocks, or at the end.
//!
//! A block that changed because a block inside it did counts by the block
//! inside. The layout marks only top-level blocks and the blocks inside a
//! `<details>`, so a change inside a list, a quote or an alert counts as a
//! change of the innermost block the layout marks around it: the whole
//! list, quote or alert.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Bound, Range};

use crate::session::settings::FollowJump;
use crate::typeset::block::BlockInfo;
use crate::typeset::compare::{Comparison, Fate, Path};

/// A change in the laid-out lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The lines of the change; none for a removal, at the line that
    /// follows it.
    pub lines: Range<usize>,
    /// The rows the lines take.
    pub rows: usize,
}

/// The changes that `changes` tells of in lines that come from `blocks`,
/// one for each line, and are `heights` rows high, in order.
pub fn groups(changes: &Comparison, blocks: &[Option<BlockInfo>], heights: &[usize]) -> Vec<Group> {
    let marked: BTreeSet<&[usize]> = blocks.iter().flatten().map(BlockInfo::path).collect();
    let removed: BTreeSet<&Path> = changes.removed().collect();
    let fates: Vec<(&Path, &Fate)> = changes.fates().collect();
    let found = Found::new(changes, blocks, &marked);

    // Each change: the block whose lines show it, or for one without
    // lines, none; and the line that follows it.
    let mut found_changes: Vec<(Option<&[usize]>, usize)> = Vec::new();
    for (index, &(path, fate)) in fates.iter().enumerate() {
        let counts = match fate {
            Fate::Stayed { .. } => false,
            Fate::Added => true,
            // Only for itself: not when a block inside it changed, was
            // added or was removed.
            Fate::Changed { was } => {
                let mut inside = fates[index + 1..]
                    .iter()
                    .take_while(|(inner, _)| inner.starts_with(path));
                !inside.any(|(_, fate)| !matches!(fate, Fate::Stayed { .. }))
                    && !removed_inside(&removed, was)
            }
        };
        if counts {
            found_changes.push((found.target(path), found.after(path)));
        }
    }

    // The new block each block of the old document that changed became.
    let became: BTreeMap<&Path, &Path> = (fates.iter())
        .filter_map(|&(path, fate)| match fate {
            Fate::Changed { was } => Some((was, path)),
            _ => None,
        })
        .collect();
    for gone in &removed {
        // A block inside a removed block goes with it.
        if (1..gone.len()).any(|end| removed.contains(&gone[..end].to_vec())) {
            continue;
        }
        let at = found.following(gone);
        // The block that held it, unless the layout marks the blocks in it.
        let holder = (1..gone.len())
            .rev()
            .find_map(|end| became.get(&gone[..end].to_vec()));
        let target = holder
            .filter(|holder| !found.holds_marked(holder))
            .and_then(|holder| found.target(holder));
        found_changes.push((target, at));
    }
    let changed: BTreeSet<&[usize]> = found_changes
        .iter()
        .filter_map(|&(target, _)| target)
        .collect();
    let places = (found_changes.iter())
        .filter(|(target, _)| target.is_none())
        .map(|&(_, at)| at);

    let is_changed = |line: usize| {
        blocks[line]
            .as_ref()
            .is_some_and(|block| changed.contains(block.path()))
    };
    let mut runs: Vec<Range<usize>> = Vec::new();
    let mut open = false;
    for (line, block) in blocks.iter().enumerate() {
        match block {
            // A blank line between two blocks neither ends a run nor
            // starts one.
            None => {}
            Some(_) if is_changed(line) => match runs.last_mut() {
                Some(run) if open => run.end = line + 1,
                _ => {
                    runs.push(line..line + 1);
                    open = true;
                }
            },
            Some(_) => open = false,
        }
    }
    // A change without lines beside a run of lines is part of that run.
    let beside = |at: usize| {
        let before = (0..at).rev().find(|&line| blocks[line].is_some());
        before.is_some_and(is_changed) || (at < blocks.len() && is_changed(at))
    };
    runs.extend(
        places
            .filter(|&at| !beside(at))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|at| at..at),
    );
    runs.sort_by_key(|run| (run.start, run.end));
    runs.into_iter()
        .map(|lines| Group {
            rows: heights[lines.clone()].iter().sum(),
            lines,
        })
        .collect()
}

/// The change of `groups` that auto-follow moves the view to, as `jump`
/// chooses: the first, the last, or the one of the most rows, the first
/// of those that take as many.
pub fn choose(groups: &[Group], jump: FollowJump) -> Option<&Group> {
    match jump {
        FollowJump::First => groups.first(),
        FollowJump::Last => groups.last(),
        FollowJump::Most => (groups.iter()).reduce(|most, group| match group.rows > most.rows {
            true => group,
            false => most,
        }),
    }
}

/// Where blocks are in the laid-out lines.
struct Found<'a> {
    /// The paths of the blocks the layout marks.
    marked: &'a BTreeSet<&'a [usize]>,
    /// Each line that comes from a block, and the block's path, in order:
    /// a block comes after the blocks above it and those it is inside.
    paths: Vec<(usize, &'a [usize])>,
    /// Each line that comes from a block that stayed or changed, and the
    /// path that block had in the old document. Blocks keep their order,
    /// so these paths come in order too.
    was: Vec<(usize, &'a Path)>,
    /// How many lines there are.
    end: usize,
}

impl<'a> Found<'a> {
    /// Where the blocks of `blocks`, one for each line, are, with
    /// `changes` telling where they were and `marked` the paths among
    /// them.
    fn new(
        changes: &'a Comparison,
        blocks: &'a [Option<BlockInfo>],
        marked: &'a BTreeSet<&'a [usize]>,
    ) -> Self {
        let paths: Vec<(usize, &[usize])> = (blocks.iter().enumerate())
            .filter_map(|(line, block)| Some((line, block.as_ref()?.path())))
            .collect();
        let was = (paths.iter())
            .filter_map(|&(line, path)| match changes.fate(path)? {
                Fate::Stayed { was } | Fate::Changed { was } => Some((line, was)),
                Fate::Added => None,
            })
            .collect();
        Self {
            marked,
            paths,
            was,
            end: blocks.len(),
        }
    }

    /// The block whose lines show a change of the block at `path`: the
    /// block itself when the layout marks it, or else the innermost block
    /// around it that the layout marks, when it marks none inside that
    /// block. `None` for a block that has no lines among blocks the layout
    /// marks, such as hidden front matter.
    fn target(&self, path: &[usize]) -> Option<&'a [usize]> {
        if let Some(&path) = self.marked.get(path) {
            return Some(path);
        }
        let around = (1..path.len())
            .rev()
            .find_map(|end| self.marked.get(&path[..end]).copied())?;
        (!self.holds_marked(around)).then_some(around)
    }

    /// Whether the layout marks a block inside the block at `path`.
    fn holds_marked(&self, path: &[usize]) -> bool {
        (self.marked)
            .range::<[usize], _>((Bound::Excluded(path), Bound::Unbounded))
            .next()
            .is_some_and(|inner| inner.starts_with(path))
    }

    /// The first line after the block at `path`, which has none of its
    /// own: the first line of a block after it, or the end.
    fn after(&self, path: &[usize]) -> usize {
        let at = (self.paths).partition_point(|&(_, block)| block <= path);
        self.paths.get(at).map_or(self.end, |&(line, _)| line)
    }

    /// The first line after the place of the removed block `gone` of the
    /// old document: the first line of a block that stayed or changed from
    /// a block after it, or the end.
    fn following(&self, gone: &Path) -> usize {
        let at = (self.was).partition_point(|&(_, was)| was <= gone);
        self.was.get(at).map_or(self.end, |&(line, _)| line)
    }
}

/// Whether one of the paths `removed` is inside the block at `path`.
fn removed_inside(removed: &BTreeSet<&Path>, path: &Path) -> bool {
    (removed.range::<Path, _>((Bound::Excluded(path), Bound::Unbounded)))
        .next()
        .is_some_and(|inner| inner.starts_with(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{FrontMatterStyle, Inputs};
    use crate::typeset::caps::Capabilities;
    use crate::typeset::compare::compare;
    use crate::typeset::layout::{Content, Stream, content};
    use crate::typeset::parse::parse;
    use crate::typeset::style;

    /// The changes from `old` to `new`, laid out with `inputs`: each the
    /// text of its lines that are not blank, a bar between them, or for a
    /// change without lines, `at` and the text of the line that follows
    /// it; and the rows it takes.
    fn changes_with(inputs: Inputs, old: &str, new: &str) -> Vec<(String, usize)> {
        let (old, new) = (parse(old), parse(new));
        let caps = inputs.caps.clone();
        let Stream { lines, blocks } = content(Content::Document(&new), &inputs);
        let places: Vec<Option<BlockInfo>> = blocks.places(&lines);
        let heights: Vec<usize> = (lines.iter())
            .map(|line| usize::from(crate::present::output::rows(line, &caps)))
            .collect();
        let text = |line: usize| match lines.get(line) {
            Some(line) => style::text(&line.spans).trim().to_owned(),
            None => "the end".to_owned(),
        };
        groups(&compare(&old, &new), &places, &heights)
            .into_iter()
            .map(|group| {
                let shown = match group.lines.is_empty() {
                    true => format!("at {}", text(group.lines.start)),
                    false => (group.lines.clone())
                        .map(text)
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join(" | "),
                };
                (shown, group.rows)
            })
            .collect()
    }

    fn changes_in(caps: Capabilities, old: &str, new: &str) -> Vec<(String, usize)> {
        changes_with(Inputs::new(caps), old, new)
    }

    fn changes(old: &str, new: &str) -> Vec<(String, usize)> {
        changes_in(Capabilities::ansi(60), old, new)
    }

    fn change(text: &str, rows: usize) -> (String, usize) {
        (text.to_owned(), rows)
    }

    #[test]
    fn an_unchanged_document_has_no_changes() {
        let source = "# Title\n\none\n\n- a\n- b\n";
        assert_eq!(changes(source, source), []);
    }

    #[test]
    fn a_block_that_changed_is_a_change() {
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\nTWO\n\nthree\n");
        assert_eq!(changed, [change("TWO", 1)]);
    }

    #[test]
    fn blocks_changed_and_added_next_to_each_other_are_one_change() {
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\nTWO\n\nnew\n\nthree\n");
        // The blank line between them counts too.
        assert_eq!(changed, [change("TWO | new", 3)]);
    }

    #[test]
    fn changes_apart_are_told_apart_in_order() {
        let changed = changes(
            "one\n\ntwo\n\nthree\n\nfour\n",
            "ONE\n\ntwo\n\nthree\n\nfour\n\nfive\n",
        );
        assert_eq!(changed, [change("ONE", 1), change("five", 1)]);
    }

    #[test]
    fn a_change_counts_every_row_of_its_lines() {
        let old = "one\n\ntwo\n";
        let new =
            "one\n\nA paragraph long enough to wrap onto a second line of the narrow window.\n";
        let changed = changes_in(Capabilities::ansi(40), old, new);
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert!(changed[0].1 >= 2, "{changed:?}");
        // A heading in kitty's larger text takes the rows of its scale.
        let changed = changes_in(Capabilities::kitty(60), "one\n", "# Big\n\none\n");
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert!(changed[0].1 >= 2, "{changed:?}");
    }

    #[test]
    fn a_removal_alone_is_a_change_of_no_lines_where_it_was() {
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\nthree\n");
        assert_eq!(changed, [change("at three", 0)]);
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\ntwo\n");
        assert_eq!(changed, [change("at the end", 0)]);
    }

    #[test]
    fn a_removal_beside_a_change_is_part_of_it() {
        // The heading is gone and the paragraph after it changed.
        let changed = changes("one\n\n# Head\n\ntwo\n\nthree\n", "one\n\nTWO\n\nthree\n");
        assert_eq!(changed, [change("TWO", 1)]);
        // A heading added where a paragraph was.
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\n# Two\n\nthree\n");
        assert_eq!(changed, [change("Two", 1)]);
    }

    #[test]
    fn a_change_in_a_details_counts_by_the_block_in_it() {
        let old = "<details>\n<summary>S</summary>\n\nfirst\n\nsecond\n\n</details>\n";
        let new = "<details>\n<summary>S</summary>\n\nfirst\n\nSECOND\n\n</details>\n";
        assert_eq!(changes(old, new), [change("SECOND", 1)]);
    }

    #[test]
    fn a_new_summary_is_a_change_of_its_line() {
        let old = "<details>\n<summary>Old</summary>\n\nbody\n\n</details>\n";
        let new = "<details>\n<summary>New</summary>\n\nbody\n\n</details>\n";
        let changed = changes(old, new);
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert!(changed[0].0.contains("New"), "{changed:?}");
        assert!(!changed[0].0.contains("body"), "{changed:?}");
    }

    #[test]
    fn a_block_removed_from_a_details_is_a_change_where_it_was() {
        let old = "<details>\n<summary>S</summary>\n\nfirst\n\nsecond\n\nthird\n\n</details>\n";
        let new = "<details>\n<summary>S</summary>\n\nfirst\n\nthird\n\n</details>\n";
        assert_eq!(changes(old, new), [change("at third", 0)]);
    }

    #[test]
    fn a_change_inside_a_list_or_a_quote_counts_the_whole_of_it() {
        let changed = changes("one\n\n- a\n- b\n\ntwo\n", "one\n\n- a\n- B\n\ntwo\n");
        assert_eq!(changed, [change("• a | • B", 2)]);
        let changed = changes("one\n\n- a\n- b\n\ntwo\n", "one\n\n- a\n\ntwo\n");
        assert_eq!(changed, [change("• a", 1)]);
        let changed = changes("one\n\n> a\n>\n> b\n\ntwo\n", "one\n\n> a\n>\n> c\n\ntwo\n");
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert!(changed[0].0.contains('c'), "{changed:?}");
    }

    #[test]
    fn a_block_without_lines_is_a_change_where_it_is() {
        // Hidden front matter lays out no lines.
        let inputs = Inputs {
            front_matter: FrontMatterStyle::Hide,
            ..Inputs::new(Capabilities::ansi(60))
        };
        let changed = changes_with(inputs, "one\n", "---\na: 1\n---\none\n");
        assert_eq!(changed, [change("at one", 0)]);
    }

    fn group(start: usize, end: usize, rows: usize) -> Group {
        Group {
            lines: start..end,
            rows,
        }
    }

    #[test]
    fn first_and_last_choose_by_place() {
        let groups = [group(2, 3, 1), group(10, 10, 0), group(20, 25, 5)];
        assert_eq!(choose(&groups, FollowJump::First), Some(&groups[0]));
        assert_eq!(choose(&groups, FollowJump::Last), Some(&groups[2]));
        assert_eq!(choose(&[], FollowJump::First), None);
    }

    #[test]
    fn most_chooses_the_change_of_the_most_rows_and_the_first_of_a_tie() {
        let groups = [
            group(2, 3, 1),
            group(5, 7, 4),
            group(9, 11, 4),
            group(20, 20, 0),
        ];
        assert_eq!(choose(&groups, FollowJump::Most), Some(&groups[1]));
        // A removal counts as no rows, yet is chosen when it is alone.
        let groups = [group(4, 4, 0)];
        assert_eq!(choose(&groups, FollowJump::Most), Some(&groups[0]));
        assert_eq!(choose(&[], FollowJump::Most), None);
    }
}
