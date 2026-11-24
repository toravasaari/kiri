use super::*;
use crate::typeset::layout::summary as cell;
use crate::typeset::model::Document;
use crate::typeset::parse::parse;
use crate::typeset::reading::page::summary as pixel;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn text(text: &str) -> Vec<Inline> {
    vec![Inline::Text(text.into())]
}

fn item(checked: Option<bool>, blocks: Vec<Block>) -> ListItem {
    ListItem { checked, blocks }
}

fn list(start: Option<u64>, items: Vec<ListItem>) -> Block {
    Block::List(crate::typeset::model::List {
        start,
        tight: true,
        footnotes: false,
        items,
    })
}

#[test]
fn the_inlines_of_a_centred_wrapper_are_those_of_the_blocks_inside_it() {
    let block = Block::Centered(vec![
        Block::Paragraph(text("first")),
        Block::Heading {
            level: 2,
            content: text("second"),
        },
    ]);
    let runs: Vec<&[Inline]> = inlines(&block);
    let expected = [text("first"), text("second")];
    assert_eq!(runs, expected.iter().map(Vec::as_slice).collect::<Vec<_>>());
}

#[test]
fn the_inlines_of_a_details_are_its_summary_and_then_those_of_the_blocks_inside_it() {
    let block = Block::Details {
        summary: text("summary"),
        blocks: vec![
            Block::Paragraph(text("first")),
            Block::BlockQuote(vec![list(
                None,
                vec![
                    item(None, vec![Block::Paragraph(text("one"))]),
                    item(None, vec![Block::Paragraph(text("two"))]),
                ],
            )]),
            Block::Heading {
                level: 2,
                content: text("last"),
            },
        ],
    };
    let runs: Vec<&[Inline]> = inlines(&block);
    let expected = [
        text("summary"),
        text("first"),
        text("one"),
        text("two"),
        text("last"),
    ];
    assert_eq!(runs, expected.iter().map(Vec::as_slice).collect::<Vec<_>>());
}

#[test]
fn the_slots_of_a_quote_with_lists_in_it_tell_depth_numbering_and_boxes() {
    let inner = list(
        None,
        vec![
            item(Some(true), vec![Block::Paragraph(text("done"))]),
            item(Some(false), vec![Block::Paragraph(text("open"))]),
        ],
    );
    let outer = list(Some(1), vec![item(None, vec![inner]), item(None, vec![])]);
    let slots = slots(&Block::BlockQuote(vec![outer]));
    let slot = |depth, numbered, checked| Slot {
        depth,
        numbered,
        checked,
    };
    assert_eq!(
        slots,
        [
            slot(0, true, None),
            slot(1, false, Some(true)),
            slot(1, false, Some(false)),
            slot(0, true, None),
        ]
    );
}

/// What each layout says of each block of each document, in each
/// setup that matters to it, as a difference where they do not agree.
struct Found {
    /// The block, as `file#n (kind)`.
    block: String,
    field: Field,
    at: String,
    cell: String,
    pixel: String,
}

/// The facts the two sides do not agree on, each as its field, its
/// case and what each side says, `(no fact)` where it says nothing.
fn pair(cell: &[Fact], pixel: &[Fact]) -> Vec<(Field, String, String, String)> {
    // A fact is the same one on both sides by its field and case; the
    // second `mark` of a block is the second on the other side too.
    let mut rest: Vec<&Fact> = pixel.iter().collect();
    let mut out = Vec::new();
    let none = || "(no fact)".to_owned();
    for a in cell {
        let at = rest
            .iter()
            .position(|b| (b.field, &b.at) == (a.field, &a.at));
        let b = at.map(|at| rest.remove(at));
        if b != Some(a) {
            out.push((
                a.field,
                a.at.clone(),
                a.value.clone(),
                b.map_or_else(none, |b| b.value.clone()),
            ));
        }
    }
    for b in rest {
        out.push((b.field, b.at.clone(), none(), b.value.clone()));
    }
    out
}

fn compare(paths: &[PathBuf]) -> Vec<Found> {
    // One thread per document, and the findings joined in the order of
    // the documents, so the report reads as it did one document at a time.
    std::thread::scope(|scope| {
        let threads: Vec<_> = paths
            .iter()
            .map(|path| scope.spawn(move || compare_document(path)))
            .collect();
        threads
            .into_iter()
            .flat_map(|thread| {
                // A panic keeps its message, as it did with no threads.
                thread
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}

fn compare_document(path: &Path) -> Vec<Found> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    let source = std::fs::read_to_string(path).unwrap();
    let name = path.strip_prefix(root).unwrap().display().to_string();
    let Document { blocks, .. } = parse(&source);
    for (number, block) in blocks.iter().enumerate() {
        for setup in Setup::TABLES {
            let (cell, pixel) = (cell::summary(block, &setup), pixel::summary(block, &setup));
            // Only a table's facts change with the setup.
            let kept = |fact: &Fact| {
                setup.is_default() || matches!(fact.field, Field::TableRows | Field::TableFrame)
            };
            let (cell, pixel): (Vec<_>, Vec<_>) = (
                cell.into_iter().filter(kept).collect(),
                pixel.into_iter().filter(kept).collect(),
            );
            let label = format!("{name}#{number} ({})", block.kind());
            for (field, at, cell, pixel) in pair(&cell, &pixel) {
                found.push(Found {
                    block: label.clone(),
                    field,
                    at,
                    cell,
                    pixel,
                });
            }
        }
    }
    found
}

/// What the test says is wrong with the list, or `None`.
fn judge(found: &[Found], list: &[Difference]) -> Option<String> {
    judge_refs(found, &list.iter().collect::<Vec<_>>())
}

fn judge_refs(found: &[Found], list: &[&Difference]) -> Option<String> {
    let mut report = String::new();
    let listed = |f: &Found| {
        list.iter().position(|d| {
            (d.field, d.at, d.cell, d.pixel)
                == (f.field, f.at.as_str(), f.cell.as_str(), f.pixel.as_str())
        })
    };
    let mut seen = BTreeSet::new();
    // Each difference not on the list once, with the blocks that show it.
    let mut unlisted: Vec<(&Found, Vec<&str>)> = Vec::new();
    for f in found {
        match listed(f) {
            Some(index) => {
                seen.insert(index);
            }
            None => match unlisted.iter_mut().find(|(g, _)| {
                (g.field, &g.at, &g.cell, &g.pixel) == (f.field, &f.at, &f.cell, &f.pixel)
            }) {
                Some((_, blocks)) => blocks.push(&f.block),
                None => unlisted.push((f, vec![&f.block])),
            },
        }
    }
    for (f, blocks) in unlisted {
        writeln!(
            report,
            "not on the list: {:?} at {:?}: cell {:?}, pixel {:?}; {} blocks, such as {}",
            f.field,
            f.at,
            f.cell,
            f.pixel,
            blocks.len(),
            blocks[..blocks.len().min(3)].join(", ")
        )
        .unwrap();
    }
    for (index, d) in list.iter().enumerate() {
        if !seen.contains(&index) {
            writeln!(
                report,
                "on the list, but no block shows it: {:?} at {:?}: cell {:?}, pixel {:?} \
                     (cell: {}; pixel: {})",
                d.field, d.at, d.cell, d.pixel, d.cell_where, d.pixel_where
            )
            .unwrap();
        }
    }
    (!report.is_empty()).then_some(report)
}

#[test]
fn the_layouts_differ_only_where_the_list_says() {
    let found = compare(&crate::testing::documents());
    if let Some(report) = judge(&found, DIFFERENCES) {
        panic!("the layouts and the list of differences disagree:\n{report}");
    }
    // Each entry is what keeps its difference from failing the test:
    // without it the message says what each layout decided.
    for (index, entry) in DIFFERENCES.iter().enumerate() {
        let rest: Vec<&Difference> = (DIFFERENCES.iter().enumerate())
            .filter(|(other, _)| *other != index)
            .map(|(_, entry)| entry)
            .collect();
        let report = judge_refs(&found, &rest).expect("a difference without its entry fails");
        let shown = format!(
            "not on the list: {:?} at {:?}: cell {:?}, pixel {:?}",
            entry.field, entry.at, entry.cell, entry.pixel
        );
        assert!(report.contains(&shown), "{shown} not in\n{report}");
    }
}

#[test]
fn the_list_has_no_entry_twice_and_the_known_drifts_are_on_it() {
    let key = |d: &Difference| (d.field, d.at, d.cell, d.pixel);
    let keys: BTreeSet<String> = DIFFERENCES
        .iter()
        .map(|d| format!("{:?}", key(d)))
        .collect();
    assert_eq!(keys.len(), DIFFERENCES.len(), "an entry twice");
    let drift = |field: Field, at: &str| {
        (DIFFERENCES.iter())
            .any(|d| d.field == field && d.at.starts_with(at) && d.status == Status::Drift)
    };
    for (field, at) in [
        (Field::HeadingSize, "level 5"),
        (Field::ListMarker, "depth 0 numbered task"),
        (Field::QuoteText, "first paragraph"),
        (Field::TableRows, "Lines"),
        (Field::TableFrame, "Grid"),
    ] {
        assert!(
            drift(field, at),
            "{field:?} at {at:?} is not a drift on the list"
        );
    }
}

#[test]
fn a_drift_names_a_bug_the_roadmap_has_and_a_difference_that_is_meant_names_none() {
    let roadmap =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/roadmap.md"))
            .unwrap();
    // Which bug decides which field, as the roadmap sorts them.
    let decided_by = |d: &Difference| match (d.field, d.at) {
        (Field::HeadingRule | Field::HeadingSize | Field::QuoteText, _) => "B17",
        (Field::ListMarker, "depth 0 numbered task item") => "B17",
        (Field::TableRows | Field::TableFrame, _) => "B18",
        (field, at) => panic!("{field:?} at {at:?} is on no bug's list"),
    };
    for d in DIFFERENCES {
        // A table too narrow for its frame is stacked on purpose.
        assert_eq!(
            d.status == Status::Intended,
            d.cell == "stacked",
            "{:?} at {:?}: cell {:?}",
            d.field,
            d.at,
            d.cell
        );
        if d.status == Status::Drift {
            assert_eq!(
                d.bug,
                Some(decided_by(d)),
                "{:?} at {:?}: cell {:?}",
                d.field,
                d.at,
                d.cell
            );
        }
        match (d.status, d.bug) {
            (Status::Drift, Some(bug)) => assert!(
                roadmap.lines().any(|line| line
                    .strip_prefix("#### ")
                    .is_some_and(|title| title.split(' ').next() == Some(bug))),
                "{:?} at {:?} names {bug}, which is no heading of the roadmap",
                d.field,
                d.at
            ),
            (Status::Intended, None) => {}
            (status, bug) => panic!(
                "{:?} at {:?} is {status:?} and names {bug:?}",
                d.field, d.at
            ),
        }
    }
}

/// The value of the cell layout's fact of `field` for the one block of
/// `markdown`.
fn cell_value(markdown: &str, field: Field) -> String {
    let block = parse(markdown).blocks.remove(0);
    let facts = cell::summary(&block, &Setup::DEFAULT);
    (facts.into_iter().find(|fact| fact.field == field))
        .unwrap_or_else(|| panic!("no {field:?} fact for {markdown:?}"))
        .value
}

#[test]
fn a_quote_is_read_by_its_text_not_by_a_link_or_code_that_opens_it() {
    // The cell layout leaves a quote's text in its own colour.
    for markdown in [
        "> plain text",
        "> [link](https://example.com) text",
        "> `code` text",
    ] {
        assert_eq!(cell_value(markdown, Field::QuoteText), "own", "{markdown}");
    }
}

#[test]
fn a_heading_is_read_by_its_own_size_not_by_a_script_that_opens_it() {
    assert_eq!(
        cell_value("# <sup>2</sup> head", Field::HeadingSize),
        "2.00"
    );
}

#[test]
fn a_heading_has_a_rule_only_where_one_is_drawn_not_where_its_text_has_a_line_glyph() {
    assert_eq!(cell_value("### ─ head", Field::HeadingRule), "false");
    assert_eq!(cell_value("### head", Field::HeadingRule), "false");
    assert_eq!(cell_value("## head", Field::HeadingRule), "true");
}

#[test]
fn a_fact_one_side_lacks_is_reported_as_itself_and_not_shifts_the_rest() {
    let fact = |field, value: &str| Fact::new(field, "level 1", value);
    let cell = [fact(Field::HeadingRule, "false")];
    let pixel = [
        fact(Field::HeadingSize, "2.00"),
        fact(Field::HeadingRule, "true"),
    ];
    let line = |field, cell: &str, pixel: &str| {
        (
            field,
            "level 1".to_owned(),
            cell.to_owned(),
            pixel.to_owned(),
        )
    };
    assert_eq!(
        pair(&cell, &pixel),
        [
            line(Field::HeadingRule, "false", "true"),
            line(Field::HeadingSize, "(no fact)", "2.00"),
        ]
    );
}

#[test]
fn a_heading_with_no_text_has_no_size_in_either_layout() {
    let block = parse("# ![](a.png)").blocks.remove(0);
    for facts in [
        cell::summary(&block, &Setup::DEFAULT),
        pixel::summary(&block, &Setup::DEFAULT),
    ] {
        assert!(
            facts.iter().all(|fact| fact.field != Field::HeadingSize),
            "{facts:?}"
        );
    }
}
