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
