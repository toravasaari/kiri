//! Where a reload changed the document, as ranges of the new version, and
//! which of the changes auto-follow moves the view to.
//!
//! The comparison tells by path which blocks of the new document changed
//! or were added, and which blocks of the old one were removed. A change is
//! a run of blocks that changed or were added, in the order of the
//! document, from the start of the first to the end of the last: the
//! blocks of a run touch, with no block that stayed between them. A
//! removal with no such block beside it is an empty range at the place
//! the alignment takes the removed block to, which is the start of the
//! block that now follows it, or the end of the one before it.
//!
//! A change is of the innermost block: a paragraph that changed in a list
//! or a quote is a change of the paragraph, not of the whole list or quote.
//! A block that changed only because a block inside it changed, was added
//! or was removed does not count itself and does not end a run.

use std::collections::BTreeSet;
use std::ops::{Bound, Range};

use crate::session::settings::FollowJump;
use crate::typeset::align::Align;
use crate::typeset::compare::{Comparison, Fate, Path};
use crate::typeset::position::Pos;
use crate::typeset::text::DocumentText;

/// What a block of the new document is to a run of changes.
enum Part {
    /// It changed or was added, itself.
    Counts,
    /// It stayed: a run does not go through it.
    Ends,
    /// A block that changed for what is inside it, or is inside a block
    /// that was added: the blocks inside tell.
    Passes,
}

/// The changes `changes` tells of in the new version of file `file`, which
/// `text` holds and `align` aligns the old to, in order.
pub fn ranges(
    file: usize,
    changes: &Comparison,
    align: &Align,
    text: &DocumentText,
) -> Vec<Range<Pos>> {
    let paths: Vec<&[usize]> = text.paths(file).collect();
    let removed: BTreeSet<&Path> = changes.removed().collect();
    let part = |index: usize| match changes.fate(paths[index]) {
        Some(Fate::Added) => Part::Counts,
        Some(Fate::Stayed { .. }) => Part::Ends,
        Some(Fate::Changed { was }) => {
            let path = paths[index];
            let mut inside = paths[index + 1..]
                .iter()
                .take_while(|inner| inner.starts_with(path));
            let moved =
                inside.any(|inner| !matches!(changes.fate(inner), Some(Fate::Stayed { .. })));
            match moved || removed_inside(&removed, &was) {
                true => Part::Passes,
                false => Part::Counts,
            }
        }
        None => Part::Passes,
    };
    // Each removal, as the empty range where it leaves a gap, by the block
    // before which it is in the order of the document. A block inside a
    // removed block goes with it.
    let mut gaps: Vec<(usize, Pos)> = (removed.iter())
        .filter(|gone| !(1..gone.len()).any(|end| removed.contains(&gone[..end].to_vec())))
        .map(|gone| {
            let at = align.pos(&Pos::block(file, gone));
            let before = paths.partition_point(|path| Pos::block(file, path) < at);
            (before, at)
        })
        .collect();
    gaps.sort();
    let mut gaps = gaps.into_iter().peekable();
    let mut runs: Vec<Range<Pos>> = Vec::new();
    let mut open = false;
    for index in 0..=paths.len() {
        while let Some((_, at)) = gaps.next_if(|(before, _)| *before == index) {
            // A gap beside a run is part of it.
            if !open {
                runs.push(at.clone()..at);
                open = true;
            }
        }
        if index == paths.len() {
            break;
        }
        match part(index) {
            Part::Ends => open = false,
            Part::Passes => {}
            Part::Counts => {
                let end = Pos::end(file, paths[index]);
                match runs.last_mut() {
                    Some(run) if open => run.end = run.end.clone().max(end),
                    _ => {
                        runs.push(Pos::block(file, paths[index])..end);
                        open = true;
                    }
                }
            }
        }
    }
    runs
}

/// The change of `ranges` that auto-follow moves the view to, as `jump`
/// chooses: the first, the last, or the one that takes the most `rows`, the
/// first of those that take as many.
pub fn choose(
    ranges: &[Range<Pos>],
    rows: impl Fn(&Range<Pos>) -> usize,
    jump: FollowJump,
) -> Option<&Range<Pos>> {
    match jump {
        FollowJump::First => ranges.first(),
        FollowJump::Last => ranges.last(),
        FollowJump::Most => {
            let mut most: Option<(&Range<Pos>, usize)> = None;
            for range in ranges {
                let rows = rows(range);
                if most.is_none_or(|(_, kept)| rows > kept) {
                    most = Some((range, rows));
                }
            }
            most.map(|(range, _)| range)
        }
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
    use crate::typeset::compare::compare;
    use crate::typeset::parse::parse;
    use crate::typeset::position::Within;

    /// The paths of a place: `1.0`, with `^` after it for an end.
    fn path(pos: &Pos) -> String {
        let path: Vec<String> = pos.path.iter().map(ToString::to_string).collect();
        let end = if pos.within == Within::End { "^" } else { "" };
        format!("{}{end}", path.join("."))
    }

    /// The changes from `old` to `new`, each as the paths it starts and
    /// ends at, `0..1^` for the blocks 0 and 1, or `at 2` for an empty
    /// range at the block 2, and `at 1^` after the block 1.
    fn changes(old: &str, new: &str) -> Vec<String> {
        let (old, new) = (parse(old), parse(new));
        let comparison = compare(&old, &new);
        let text = DocumentText::new([&new]);
        let align = Align::new(0, &comparison, &text);
        ranges(0, &comparison, &align, &text)
            .iter()
            .map(|range| match range.start == range.end {
                true => format!("at {}", path(&range.start)),
                false => format!("{}..{}", path(&range.start), path(&range.end)),
            })
            .collect()
    }

    #[test]
    fn an_unchanged_document_has_no_changes() {
        let source = "# Title\n\none\n\n- a\n- b\n";
        assert_eq!(changes(source, source), Vec::<String>::new());
    }

    #[test]
    fn a_block_that_changed_is_a_change() {
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\nTWO\n\nthree\n");
        assert_eq!(changed, ["1..1^"]);
    }

    #[test]
    fn blocks_changed_and_added_next_to_each_other_are_one_change() {
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\nTWO\n\nnew\n\nthree\n");
        assert_eq!(changed, ["1..2^"]);
    }

    #[test]
    fn a_block_that_stayed_ends_a_change() {
        let changed = changes(
            "one\n\ntwo\n\nthree\n\nfour\n",
            "ONE\n\ntwo\n\nthree\n\nfour\n\nfive\n",
        );
        assert_eq!(changed, ["0..0^", "4..4^"]);
    }

    #[test]
    fn a_removal_alone_is_an_empty_range_where_it_was() {
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\nthree\n");
        assert_eq!(changed, ["at 1"]);
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\ntwo\n");
        assert_eq!(changed, ["at 1^"]);
        let changed = changes("one\n\ntwo\n", "");
        assert_eq!(changed, ["at "]);
    }

    #[test]
    fn a_removal_beside_a_change_is_part_of_it() {
        // The heading is gone and the paragraph after it changed.
        let changed = changes("one\n\n# Head\n\ntwo\n\nthree\n", "one\n\nTWO\n\nthree\n");
        assert_eq!(changed, ["1..1^"]);
        // A heading added where a paragraph was.
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\n# Two\n\nthree\n");
        assert_eq!(changed, ["1..1^"]);
        // A removal after a change at the end.
        let changed = changes("one\n\ntwo\n\nthree\n", "one\n\nTWO\n");
        assert_eq!(changed, ["1..1^"]);
    }

    #[test]
    fn a_change_in_a_details_is_of_the_block_in_it() {
        let old = "<details>\n<summary>S</summary>\n\nfirst\n\nsecond\n\n</details>\n";
        let new = "<details>\n<summary>S</summary>\n\nfirst\n\nSECOND\n\n</details>\n";
        assert_eq!(changes(old, new), ["0.1..0.1^"]);
    }

    #[test]
    fn a_new_summary_is_a_change_of_the_details_itself() {
        let old = "<details>\n<summary>Old</summary>\n\nbody\n\n</details>\n";
        let new = "<details>\n<summary>New</summary>\n\nbody\n\n</details>\n";
        assert_eq!(changes(old, new), ["0..0^"].map(String::from));
    }

    #[test]
    fn a_block_removed_from_a_details_is_a_gap_where_it_was() {
        let old = "<details>\n<summary>S</summary>\n\nfirst\n\nsecond\n\nthird\n\n</details>\n";
        let new = "<details>\n<summary>S</summary>\n\nfirst\n\nthird\n\n</details>\n";
        assert_eq!(changes(old, new), ["at 0.1"]);
    }

    #[test]
    fn a_change_in_a_list_item_is_of_the_innermost_block_and_not_of_the_whole_list() {
        let changed = changes("one\n\n- a\n- b\n\ntwo\n", "one\n\n- a\n- B\n\ntwo\n");
        assert_eq!(changed, ["1.1.0..1.1.0^"]);
        // A removal from inside the list tints nothing: an empty range.
        let changed = changes("one\n\n- a\n- b\n\ntwo\n", "one\n\n- a\n\ntwo\n");
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert!(changed[0].starts_with("at "), "{changed:?}");
        let changed = changes("one\n\n> a\n>\n> b\n\ntwo\n", "one\n\n> a\n>\n> c\n\ntwo\n");
        assert_eq!(changed, ["1.1..1.1^"]);
    }

    #[test]
    fn a_stayed_item_between_two_changed_ones_makes_two_changes() {
        let changed = changes("- a\n- b\n- c\n", "- A\n- b\n- C\n");
        assert_eq!(changed, ["0.0.0..0.0.0^", "0.2.0..0.2.0^"]);
    }

    fn range(start: usize, end: usize) -> Range<Pos> {
        Pos::block(0, &[start])..Pos::end(0, &[end])
    }

    #[test]
    fn first_and_last_choose_by_place() {
        let ranges = [range(2, 2), range(10, 10), range(20, 25)];
        let rows = |_: &Range<Pos>| 1;
        assert_eq!(choose(&ranges, rows, FollowJump::First), Some(&ranges[0]));
        assert_eq!(choose(&ranges, rows, FollowJump::Last), Some(&ranges[2]));
        assert_eq!(choose(&[], rows, FollowJump::First), None);
    }

    #[test]
    fn most_chooses_the_change_of_the_most_rows_and_the_first_of_a_tie() {
        let ranges = [range(2, 2), range(5, 7), range(9, 11), range(20, 20)];
        let rows = |range: &Range<Pos>| match range.start.path[0] {
            5 | 9 => 4,
            2 => 1,
            _ => 0,
        };
        assert_eq!(choose(&ranges, rows, FollowJump::Most), Some(&ranges[1]));
        // A removal counts as no rows, yet is chosen when it is alone.
        let alone = [range(4, 4)];
        assert_eq!(choose(&alone, rows, FollowJump::Most), Some(&alone[0]));
        assert_eq!(choose(&[], rows, FollowJump::Most), None);
    }
}
