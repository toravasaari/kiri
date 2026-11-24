//! Footnotes: a reference in the text and the notes after the document, built from the anchors, links and lists that the document model already has.
//!
//! A reference such as `[^1]` is a raised link to its note, with an anchor
//! of its own for the link back. The notes come last, as GitHub lists them:
//! a rule and a list numbered in brackets, each note ending in links back to the places
//! that refer to it. comrak numbers the notes by the first reference to
//! each, moves them to the end of the document and leaves out a note that
//! nothing refers to. The ids are GitHub's: `fn-1` for a note, `fnref-1`
//! for its first reference and `fnref-1-2` for the second.

use comrak::Node;
use comrak::nodes::{NodeFootnoteDefinition, NodeFootnoteReference, NodeValue, Sourcepos};

use super::unknown::Unknown;
use super::{Block, Inline, List, ListItem, Position, blocks};

/// The reference `reference` to a note, as it is in the text: a raised link
/// to the note, after the anchor that the note's link back goes to.
pub(super) fn reference(reference: &NodeFootnoteReference) -> Inline {
    let number = reference.ix;
    Inline::Superscript(vec![
        Inline::Anchor(reference_id(number, reference.ref_num)),
        Inline::Link {
            url: format!("#{}", note_id(number)),
            title: None,
            content: vec![Inline::Text(number.to_string())],
        },
    ])
}

/// The blocks that the notes `definitions` make: a rule and the list of
/// the notes, each block with the place in the source of the notes it is
/// made of. Nothing for no notes.
pub(super) fn notes(definitions: &[Node<'_>], unknown: &mut Unknown) -> Vec<(Block, Position)> {
    let (Some(first), Some(last)) = (definitions.first(), definitions.last()) else {
        return Vec::new();
    };
    // comrak lists the notes in the order they are referred to, so they
    // are not in the order they are written: the piece of source they
    // come from runs from the first start to the last end, with other
    // blocks, and a note nothing refers to, between them.
    let starts = definitions.iter().map(|node| node.data().sourcepos.start);
    let ends = definitions.iter().map(|node| node.data().sourcepos.end);
    let whole = Sourcepos {
        start: starts
            .min_by_key(|at| (at.line, at.column))
            .unwrap_or(first.data().sourcepos.start),
        end: ends
            .max_by_key(|at| (at.line, at.column))
            .unwrap_or(last.data().sourcepos.end),
    };
    let (items, positions): (Vec<_>, Vec<_>) = definitions
        .iter()
        .zip(1u32..)
        .map(|(definition, number)| note(definition, number, unknown))
        .unzip();
    // A note of several blocks is set apart from the next as a loose list
    // does; one of a single paragraph each stay close.
    let tight = items.iter().all(|item| item.blocks.len() == 1);
    let list = Block::List(List {
        start: Some(1),
        tight,
        footnotes: true,
        items,
    });
    let line = whole.start.line;
    let rule = Position::inside(whole, line);
    let list_position = Position {
        children: positions,
        ..Position::inside(whole, line)
    };
    vec![(Block::ThematicBreak, rule), (list, list_position)]
}

/// The note `number`, written as `definition`: its blocks, with its anchor
/// first and the links back to its references at the end.
fn note(definition: Node<'_>, number: u32, unknown: &mut Unknown) -> (ListItem, Position) {
    let NodeValue::FootnoteDefinition(NodeFootnoteDefinition {
        total_references, ..
    }) = &definition.data().value
    else {
        unreachable!("a footnote definition");
    };
    let (mut blocks, mut positions): (Vec<_>, Vec<_>) =
        blocks(definition, unknown).into_iter().unzip();
    let anchor = Inline::Anchor(note_id(number));
    match blocks.first_mut() {
        Some(Block::Paragraph(inlines)) => inlines.insert(0, anchor),
        _ => {
            blocks.insert(0, Block::Paragraph(vec![anchor]));
            positions.insert(0, whole_of(definition));
        }
    }
    let links = links_back(number, *total_references);
    match blocks.last_mut() {
        Some(Block::Paragraph(inlines)) => push_links(inlines, links),
        _ => {
            blocks.push(Block::Paragraph(links));
            positions.push(whole_of(definition));
        }
    }
    let item = ListItem {
        checked: None,
        blocks,
    };
    (
        item,
        Position {
            at: None,
            children: positions,
        },
    )
}

fn whole_of(definition: Node<'_>) -> Position {
    let at = definition.data().sourcepos;
    Position::inside(at, at.start.line)
}

fn links_back(number: u32, references: u32) -> Vec<Inline> {
    (1..=references.max(1))
        .map(|nth| Inline::Link {
            url: format!("#{}", reference_id(number, nth)),
            title: None,
            content: vec![Inline::Text(back_arrow(nth))],
        })
        .collect()
}

/// The arrow of the link back to the `nth` reference: `↩`, then `↩²`,
/// `↩³`, as GitHub writes them.
fn back_arrow(nth: u32) -> String {
    const SUPERSCRIPTS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    let mut arrow = String::from('\u{21a9}');
    if nth > 1 {
        arrow.extend(
            nth.to_string()
                .chars()
                .filter_map(|digit| digit.to_digit(10))
                .map(|digit| SUPERSCRIPTS[digit as usize]),
        );
    }
    arrow
}

/// Adds `links` to the end of `inlines`: the first after a space, the
/// others each after an em space, so that the arrows are far enough apart
/// not to be taken for one another.
fn push_links(inlines: &mut Vec<Inline>, links: Vec<Inline>) {
    for (index, link) in links.into_iter().enumerate() {
        let gap = if index == 0 { ' ' } else { '\u{2003}' };
        match inlines.last_mut() {
            Some(Inline::Text(text)) => text.push(gap),
            _ => inlines.push(Inline::Text(gap.to_string())),
        }
        inlines.push(link);
    }
}

fn note_id(number: u32) -> String {
    format!("fn-{number}")
}

fn reference_id(number: u32, nth: u32) -> String {
    match nth {
        0 | 1 => format!("fnref-{number}"),
        nth => format!("fnref-{number}-{nth}"),
    }
}

#[cfg(test)]
mod tests;
