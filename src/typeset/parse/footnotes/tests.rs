use super::super::parse;
use crate::typeset::model::{Block, Inline, List};

fn text(text: &str) -> Inline {
    Inline::Text(text.into())
}

/// The reference to note `number`, as `id` names its place.
fn reference(number: u32, id: &str) -> Inline {
    Inline::Superscript(vec![
        Inline::Anchor(id.into()),
        Inline::Link {
            url: format!("#fn-{number}"),
            title: None,
            content: vec![text(&number.to_string())],
        },
    ])
}

/// The link back from a note to a reference.
fn back(id: &str, label: &str) -> Inline {
    Inline::Link {
        url: format!("#{id}"),
        title: None,
        content: vec![text(label)],
    }
}

#[test]
fn a_reference_is_a_raised_link_to_its_note_and_the_note_is_listed_after_the_text() {
    let document = parse("A[^1].\n\n[^1]: One.\n");
    assert_eq!(
        document.blocks,
        vec![
            Block::Paragraph(vec![text("A"), reference(1, "fnref-1"), text(".")]),
            Block::ThematicBreak,
            Block::List(List {
                start: Some(1),
                tight: true,
                items: vec![crate::typeset::model::ListItem {
                    checked: None,
                    blocks: vec![Block::Paragraph(vec![
                        Inline::Anchor("fn-1".into()),
                        text("One. "),
                        back("fnref-1", "[1]"),
                    ])],
                }],
            }),
        ]
    );
}

#[test]
fn notes_are_numbered_as_they_are_first_referred_to_and_a_repeat_has_an_id_of_its_own() {
    let document = parse("B[^b] A[^a] B[^b]\n\n[^a]: Note a.\n\n[^b]: Note b.\n");
    let Block::Paragraph(inlines) = &document.blocks[0] else {
        panic!("a paragraph")
    };
    assert_eq!(
        inlines,
        &[
            text("B"),
            reference(1, "fnref-1"),
            text(" A"),
            reference(2, "fnref-2"),
            text(" B"),
            reference(1, "fnref-1-2"),
        ]
    );
    let Block::List(list) = &document.blocks[2] else {
        panic!("the notes")
    };
    let Block::Paragraph(first) = &list.items[0].blocks[0] else {
        panic!("a paragraph")
    };
    assert_eq!(
        first,
        &[
            Inline::Anchor("fn-1".into()),
            text("Note b. "),
            back("fnref-1", "[1]"),
            text(" "),
            back("fnref-1-2", "[1.2]"),
        ]
    );
}

#[test]
fn a_note_that_ends_in_a_code_block_gets_its_links_back_in_a_paragraph_of_their_own() {
    let document = parse("A[^1].\n\n[^1]: Text.\n\n    ```\n    code\n    ```\n");
    let Block::List(list) = &document.blocks[2] else {
        panic!("the notes")
    };
    let blocks = &list.items[0].blocks;
    assert!(matches!(blocks[1], Block::CodeBlock { .. }));
    assert_eq!(blocks[2], Block::Paragraph(vec![back("fnref-1", "[1]")]));
}

#[test]
fn a_note_that_nothing_refers_to_and_a_reference_without_a_note_are_not_footnotes() {
    let document = parse("A[^x].\n\n[^y]: Unused.\n");
    assert_eq!(
        document.blocks,
        vec![Block::Paragraph(vec![text("A[^x].")])]
    );
}

#[test]
fn every_block_of_the_document_has_a_place_in_the_source() {
    let source = "A[^1].\n\n[^1]: One.\n\n    More.\n";
    let document = parse(source);
    // The paragraph, the rule, the list and the two paragraphs in its note.
    let note = document
        .places
        .get(&[2, 0, 1])
        .expect("the second paragraph");
    assert_eq!(&source[note.bytes.clone()], "More.");
    assert_eq!(note.line, 5);
}
