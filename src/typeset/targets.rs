//! What a document has that can be followed or drawn: its links and its
//! embeds, as stretches of its text.
//!
//! What a document holds is asked of the model, and where it is shown is
//! asked of a layout. A link or an embed is a range of places in the text of
//! the document, so it is the same one in every layout, and a layout that
//! shows it somewhere else, or not at all, says so for itself.
//!
//! This lists each link as it is written. That a terminal shows a link
//! without an address as no link at all, or two links next to each other
//! with one address as one hyperlink, is how a terminal shows links, not what
//! the document has, and is for the part that handles a terminal.
//! Whether an embed is drawn is a layout's answer as well: a diagram here is
//! a code block in a language that kiri can draw, whether or not a
//! program for it is at hand.

use std::ops::Range;

use crate::typeset::diagrams::language;
use crate::typeset::diagrams::request::INLINE_FORMULA;
use crate::typeset::model::{Block, Document, Inline};
use crate::typeset::position::Pos;
use crate::typeset::text::pieces_with;

/// A link and where its text is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub range: Range<Pos>,
    pub url: String,
}

/// An image, a formula or a diagram, and where its text is: the alt text of
/// an image, the source of a formula or a diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Embed {
    pub range: Range<Pos>,
    pub kind: EmbedKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedKind {
    /// An image in a text, or alone in a paragraph.
    Image { url: String },
    /// A formula in a text, or a code block in `math`.
    Formula,
    /// A code block in a language that kiri draws diagrams in.
    Diagram { language: String },
    /// A file shown as an image on its own: the range is its whole block,
    /// as it has no text.
    ImageFile { name: String },
}

/// The links and the embeds of the shown documents, each in the order of
/// the documents by where it starts. An image in a link is both, and the
/// ranges are the same stretch of text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Targets {
    pub links: Vec<Link>,
    pub embeds: Vec<Embed>,
}

/// The links and the embeds of `files`, shown in this order: file 0 is the
/// first.
pub fn targets<'a>(files: impl IntoIterator<Item = &'a Document>) -> Targets {
    let mut found = Targets::default();
    for (file, document) in files.into_iter().enumerate() {
        for (path, block) in document.blocks() {
            let at = |text: usize, bytes: Range<usize>| Range {
                start: Pos::text(file, &path, text, bytes.start),
                end: Pos::text(file, &path, text, bytes.end),
            };
            let pieces = pieces_with(block, &mut |text, inline, bytes| match inline {
                Inline::Link { url, .. } => found.links.push(Link {
                    range: at(text, bytes),
                    url: url.clone(),
                }),
                Inline::Image(image) => found.embeds.push(Embed {
                    range: at(text, bytes),
                    kind: EmbedKind::Image {
                        url: image.url.clone(),
                    },
                }),
                Inline::Formula { .. } => found.embeds.push(Embed {
                    range: at(text, bytes),
                    kind: EmbedKind::Formula,
                }),
                _ => {}
            });
            match block {
                Block::CodeBlock {
                    language: Some(name),
                    ..
                } if let Some(drawn) = language::language(name) => {
                    let end = pieces.first().map_or(0, |piece| piece.text.len());
                    found.embeds.push(Embed {
                        range: at(0, 0..end),
                        kind: match drawn.name {
                            "math" | INLINE_FORMULA => EmbedKind::Formula,
                            language => EmbedKind::Diagram {
                                language: language.to_owned(),
                            },
                        },
                    });
                }
                Block::ImageFile(name) => found.embeds.push(Embed {
                    range: Pos::block(file, &path)..Pos::end(file, &path),
                    kind: EmbedKind::ImageFile { name: name.clone() },
                }),
                _ => {}
            }
        }
    }
    // The blocks come in order, and the inlines of a text in the order they
    // end, an inline inside another first: by where they start, in order.
    found
        .links
        .sort_by(|a, b| a.range.start.cmp(&b.range.start));
    found
        .embeds
        .sort_by(|a, b| a.range.start.cmp(&b.range.start));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::parse::parse;
    use crate::typeset::position::Within;

    fn of(source: &str) -> Targets {
        targets([&parse(source)])
    }

    /// `text@start..end` of the places of a range of a text, with the path.
    fn shown(range: &Range<Pos>) -> String {
        match (&range.start.within, &range.end.within) {
            (Within::Text { text, offset: a }, Within::Text { offset: b, .. }) => {
                format!("{:?}:{text}@{a}..{b}", range.start.path)
            }
            (Within::Block, Within::End) => format!("{:?}:block", range.start.path),
            other => panic!("{other:?}"),
        }
    }

    fn links(source: &str) -> Vec<(String, String)> {
        (of(source).links.iter())
            .map(|link| (shown(&link.range), link.url.clone()))
            .collect()
    }

    fn embeds(source: &str) -> Vec<(String, EmbedKind)> {
        (of(source).embeds.iter())
            .map(|embed| (shown(&embed.range), embed.kind.clone()))
            .collect()
    }

    fn pair(range: &str, url: &str) -> (String, String) {
        (range.to_owned(), url.to_owned())
    }

    fn image(url: &str) -> EmbedKind {
        EmbedKind::Image { url: url.into() }
    }

    #[test]
    fn a_link_is_the_text_of_what_is_in_it() {
        assert_eq!(links("see [a *b*](u) end\n"), [pair("[0]:0@4..7", "u")]);
    }

    #[test]
    fn links_are_listed_as_written_with_or_without_an_address_and_touching() {
        assert_eq!(
            links("[a]() [b](u)**[c](u)**\n"),
            [
                pair("[0]:0@0..1", ""),
                pair("[0]:0@2..3", "u"),
                pair("[0]:0@3..4", "u"),
            ]
        );
    }

    #[test]
    fn links_in_a_summary_a_table_and_a_quote_have_their_blocks() {
        let source = "<details>\n<summary><a href=\"us\"><img src=\"s.png\" alt=\"s\"></a></summary>\n\n> [q](uq)\n\n</details>\n\n| [t](ut) |\n|---|\n";
        assert_eq!(
            links(source),
            [
                pair("[0]:0@0..1", "us"),
                pair("[0, 0, 0]:0@0..1", "uq"),
                pair("[1]:0@0..1", "ut"),
            ]
        );
        // An alert's title is not marked up, so it holds no link.
        assert!(links("> [!NOTE] [a](u)\n> text\n").is_empty());
    }

    #[test]
    fn an_image_in_text_a_lone_image_and_a_linked_one() {
        assert_eq!(
            embeds("a ![alt](a.png) b\n\n![lone](b.png)\n\n[![both](c.png)](u)\n"),
            [
                ("[0]:0@2..5".to_owned(), image("a.png")),
                ("[1]:0@0..4".to_owned(), image("b.png")),
                ("[2]:0@0..4".to_owned(), image("c.png")),
            ]
        );
        let both = of("[![both](c.png)](u)\n");
        assert_eq!(both.links[0].range, both.embeds[0].range);
    }

    #[test]
    fn a_formula_in_text_and_a_math_block_are_formulas_over_their_source() {
        assert_eq!(
            embeds("the $x^2$ here\n\n```math\na + b\n```\n"),
            [
                ("[0]:0@4..7".to_owned(), EmbedKind::Formula),
                ("[1]:0@0..5".to_owned(), EmbedKind::Formula),
            ]
        );
    }

    #[test]
    fn a_code_block_is_a_diagram_when_kiri_knows_its_language_and_not_else() {
        assert_eq!(
            embeds("```mermaid\ngraph TD\n```\n\n```rust\nlet a;\n```\n\n```\nplain\n```\n"),
            [(
                "[0]:0@0..8".to_owned(),
                EmbedKind::Diagram {
                    language: "mermaid".into()
                }
            )]
        );
    }

    #[test]
    fn the_language_of_a_code_block_is_read_as_kiri_reads_it() {
        let found = |language: &str| embeds(&format!("```{language}\nx\n```\n"));
        let diagram = EmbedKind::Diagram {
            language: "mermaid".into(),
        };
        // A formula, whatever it is called, is never a diagram.
        for language in ["math", "MATH", "math-inline"] {
            assert_eq!(
                found(language),
                [("[0]:0@0..1".to_owned(), EmbedKind::Formula)]
            );
        }
        assert_eq!(found("Mermaid"), [("[0]:0@0..1".to_owned(), diagram)]);
    }

    #[test]
    fn an_image_file_is_the_range_of_its_block_and_its_files_follow_one_another() {
        let file = Document::new(vec![Block::ImageFile("a.png".into())]);
        let text = parse("![x](y.png)\n");
        let found = targets([&file, &text]);
        assert_eq!(found.embeds.len(), 2);
        assert_eq!(
            found.embeds[0].range,
            Pos::block(0, &[0])..Pos::end(0, &[0])
        );
        assert_eq!(
            found.embeds[0].kind,
            EmbedKind::ImageFile {
                name: "a.png".into()
            }
        );
        assert_eq!(found.embeds[1].range.start.file, 1);
    }

    #[test]
    fn what_is_in_a_text_is_not_a_target_and_the_places_come_in_order() {
        let found = of("# [h](u) ![i](i.png)\n\ntext\n\n- [l](v)\n");
        assert!(
            found
                .links
                .windows(2)
                .all(|p| p[0].range.start < p[1].range.start)
        );
        assert_eq!(found.links.len(), 2);
        assert_eq!(found.embeds.len(), 1);
        assert!(of("plain text\n").links.is_empty());
    }
}
