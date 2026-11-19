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
use std::path::Path;

use crate::typeset::anchors::{Ids, anchor_text, decode};
use crate::typeset::diagrams::language;
use crate::typeset::diagrams::request::INLINE_FORMULA;
use crate::typeset::input::is_markdown;
use crate::typeset::model::{Block, Document, Inline};
use crate::typeset::parse::{anchor_ids, html_headings};
use crate::typeset::position::Pos;
use crate::typeset::text::pieces_with;

/// A link and where its text is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub range: Range<Pos>,
    pub url: String,
    /// The title written after the address: `[a](u "title")`.
    pub title: Option<String>,
    pub goes: Goes,
}

/// Where a link goes, as its address says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Goes {
    /// No address: `[a]()`.
    Nowhere,
    /// `#id`: a place in the link's own file. An empty `id` is the start.
    Here { id: String },
    /// A path with no scheme, relative or absolute, with or without `#id`.
    File {
        path: String,
        id: Option<String>,
        /// Whether the path has a Markdown extension.
        markdown: bool,
    },
    /// An address with a scheme or a host: `https:`, `mailto:`, `//host`.
    Away,
}

/// Where a link to `url` goes: told by the address alone, without looking
/// at any file.
pub fn goes(url: &str) -> Goes {
    if url.is_empty() {
        return Goes::Nowhere;
    }
    if let Some(id) = url.strip_prefix('#') {
        return Goes::Here {
            id: decode(id).into_owned(),
        };
    }
    if url.starts_with("//") {
        return Goes::Away;
    }
    match scheme(url) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            // `file:///a/b`, `file://host/a/b` and `file:/a/b`: the path
            // starts at the first `/` after the host.
            let rest = &url[scheme.len() + 1..];
            let path = match rest.strip_prefix("//") {
                Some(after) => after.find('/').map_or("", |at| &after[at..]),
                None => rest,
            };
            file(path)
        }
        Some(_) => Goes::Away,
        None => file(url),
    }
}

/// The scheme of `url`: letters, digits, `+`, `.` and `-` from a letter up
/// to a `:`. One letter is a drive of Windows, as in `C:\a.md`.
fn scheme(url: &str) -> Option<&str> {
    let end = url.find(':')?;
    let name = &url[..end];
    let mut chars = name.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c));
    (valid && end > 1).then_some(name)
}

/// A link to the file `address`, a path with a query and a fragment.
fn file(address: &str) -> Goes {
    let (path, id) = match address.split_once('#') {
        Some((path, id)) => (
            path,
            Some(decode(id).into_owned()).filter(|id| !id.is_empty()),
        ),
        None => (address, None),
    };
    let path = path.split('?').next().unwrap_or("");
    let path = decode(path).into_owned();
    let markdown = is_markdown(Path::new(&path));
    Goes::File { path, id, markdown }
}

/// A place a link can go to by its id: a heading, or an `<a id>` or
/// `<a name>` of HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// The heading or the block of HTML, or the empty place in a text where
    /// an inline tag is.
    pub range: Range<Pos>,
    pub id: String,
    pub kind: NamedKind,
    /// The level and text of a heading that is written in a block of
    /// HTML, such as `<h2>` in a `<div>`, where the block holds no
    /// heading to read them from; `None` for the rest.
    pub inner: Option<Inner>,
}

/// The level and the plain text of a heading in a block of HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inner {
    pub level: u8,
    /// As [`anchor_text`] gives it.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedKind {
    Heading,
    Html,
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
    /// The places that have an id, in the order of the documents:
    /// `range.start.file` is the file.
    pub named: Vec<Named>,
}

impl Targets {
    /// The place named `id` in file `file`: the one written exactly as
    /// `id`, and else the first in lower case, as GitHub's own script finds
    /// `#Installation` at `installation`. A heading's id is in lower case.
    pub fn named(&self, file: usize, id: &str) -> Option<&Named> {
        let of_file = || {
            self.named
                .iter()
                .filter(|named| named.range.start.file == file)
        };
        of_file().find(|named| named.id == id).or_else(|| {
            let lower = id.to_lowercase();
            of_file().find(|named| named.id == lower)
        })
    }
}

/// The headings written in the block of HTML `source`, as places in its
/// text. A heading is named by its `id`, and else as a Markdown heading
/// is, with the same numbering in `ids`.
fn html_headings_named(
    source: &str,
    ids: &mut Ids,
    at: &impl Fn(usize, Range<usize>) -> Range<Pos>,
) -> Vec<Named> {
    html_headings(source)
        .into_iter()
        .map(|inner| Named {
            range: at(0, inner.bytes.clone()),
            id: inner.id.unwrap_or_else(|| ids.of(&inner.content)),
            kind: NamedKind::Heading,
            inner: Some(Inner {
                level: inner.level,
                text: anchor_text(&inner.content),
            }),
        })
        .collect()
}

/// The links, the embeds and the named places of `files`, shown in this order: file 0 is the
/// first.
pub fn targets<'a>(files: impl IntoIterator<Item = &'a Document>) -> Targets {
    let mut found = Targets::default();
    for (file, document) in files.into_iter().enumerate() {
        // The ids of the headings of a file are numbered within the file.
        let mut ids = Ids::new();
        for (path, block) in document.blocks() {
            let at = |text: usize, bytes: Range<usize>| Range {
                start: Pos::text(file, &path, text, bytes.start),
                end: Pos::text(file, &path, text, bytes.end),
            };
            // The cells of a table of a CSV's source are only text, with no
            // link or embed in them: they are not made to be looked through.
            if matches!(block, Block::Table(table) if table.source().is_some()) {
                continue;
            }
            let pieces = pieces_with(block, &mut |text, inline, bytes| match inline {
                Inline::Link { url, title, .. } => found.links.push(Link {
                    range: at(text, bytes),
                    url: url.clone(),
                    title: title.clone(),
                    goes: goes(url),
                }),
                Inline::Html(tag) => {
                    let here = at(text, bytes.start..bytes.start);
                    found
                        .named
                        .extend(anchor_ids(tag).into_iter().map(|id| Named {
                            range: here.clone(),
                            id,
                            kind: NamedKind::Html,
                            inner: None,
                        }));
                }
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
            let whole = || Pos::block(file, &path)..Pos::end(file, &path);
            match block {
                Block::Heading { content, .. } => found.named.push(Named {
                    range: whole(),
                    id: ids.of(content),
                    kind: NamedKind::Heading,
                    inner: None,
                }),
                Block::Html(source) => {
                    found
                        .named
                        .extend(html_headings_named(source, &mut ids, &at));
                    found
                        .named
                        .extend(anchor_ids(source).into_iter().map(|id| Named {
                            range: whole(),
                            id,
                            kind: NamedKind::Html,
                            inner: None,
                        }));
                }
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
        .named
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
        // An alert's title is not marked up, so it holds no link; its body
        // is blocks, and a link in one is found.
        assert!(links("> [!NOTE] [a](u)\n> text\n").is_empty());
        assert_eq!(
            links("> [!NOTE]\n> see [a](u)\n"),
            [pair("[0, 0]:0@4..5", "u")]
        );
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

    fn named(source: &str) -> Vec<(usize, String, NamedKind, String)> {
        (of(source).named.iter())
            .map(|named| {
                (
                    named.range.start.file,
                    named.id.clone(),
                    named.kind,
                    shown(&named.range),
                )
            })
            .collect()
    }

    fn here(id: &str) -> Goes {
        Goes::Here { id: id.into() }
    }

    fn file_goes(path: &str, id: Option<&str>, markdown: bool) -> Goes {
        Goes::File {
            path: path.into(),
            id: id.map(Into::into),
            markdown,
        }
    }

    #[test]
    fn headings_are_named_places_with_their_ids_in_every_file() {
        let a = parse("# Setup\n\n## Use\n");
        let b = parse("# Setup\n\ntext\n\n# Setup\n");
        let found = targets([&a, &b]);
        let ids: Vec<_> = (found.named.iter())
            .map(|named| (named.range.start.file, named.id.as_str()))
            .collect();
        assert_eq!(
            ids,
            [(0, "setup"), (0, "use"), (1, "setup"), (1, "setup-1")]
        );
        assert!(found.named.iter().all(|n| n.kind == NamedKind::Heading));
        assert_eq!(found.named(1, "setup-1").unwrap().range.start.path, [2]);
        assert_eq!(found.named(1, "use"), None);
    }

    #[test]
    fn headings_in_a_quote_a_list_and_details_are_named_too() {
        let source = "> # Quoted\n\n- # Listed\n\n<details>\n<summary>S</summary>\n\n## Inside\n\n</details>\n\n<h2>Html heading</h2>\n";
        let ids: Vec<_> = named(source).into_iter().map(|n| n.1).collect();
        assert_eq!(ids, ["quoted", "listed", "inside", "html-heading"]);
    }

    #[test]
    fn headings_inside_a_block_of_html_are_named_places() {
        let ids = |source: &str| -> Vec<(String, NamedKind)> {
            named(source).into_iter().map(|n| (n.1, n.2)).collect()
        };
        let heading = |id: &str| (id.to_owned(), NamedKind::Heading);
        assert_eq!(ids("<div>\n<h2>In div</h2>\n</div>\n"), [heading("in-div")]);
        // The `id` attribute is the id, and none is made of the text.
        assert_eq!(
            ids("<div>\n<h3 id=\"x\">Three</h3>\n</div>\n"),
            [heading("x")]
        );
        // Two in one block, and the same text as a Markdown heading before.
        assert_eq!(
            ids("# Same\n\n<div>\n<h2>Same</h2>\n<h4>Other</h4>\n<h2>Same</h2>\n</div>\n"),
            [
                heading("same"),
                heading("same-1"),
                heading("other"),
                heading("same-2")
            ]
        );
        // In a `<details>` of HTML.
        assert_eq!(
            ids(
                "<details>\n<summary>S</summary>\n\n<div>\n<h2>Inside</h2>\n</div>\n\n</details>\n"
            ),
            [heading("inside")]
        );
        // A tag with no end, and a heading in a heading, are none.
        assert_eq!(ids("<div>\n<h2>Open\n</div>\n"), []);
    }

    #[test]
    fn a_link_finds_a_heading_inside_html_by_its_id() {
        let found =
            of("<div align=\"center\">\n<h2 id=\"top\">Top</h2>\n<h2>In div</h2>\n</div>\n");
        assert_eq!(found.named(0, "top").unwrap().kind, NamedKind::Heading);
        assert!(found.named(0, "in-div").is_some());
        assert_eq!(found.named(0, "In-Div").unwrap().id, "in-div");
    }

    #[test]
    fn an_a_id_and_an_a_name_in_text_and_alone_are_named_places() {
        assert_eq!(
            named("text <a id=\"x\"></a> more <a name=\"y\">z</a>\n"),
            [
                (0, "x".into(), NamedKind::Html, "[0]:0@5..5".into()),
                (0, "y".into(), NamedKind::Html, "[0]:0@25..25".into()),
            ]
        );
        // A tag alone on its line is an HTML block, and its range the block's.
        assert_eq!(
            named("<a name=\"top\">\n\n# Title\n"),
            [
                (0, "top".into(), NamedKind::Html, "[0]:block".into()),
                (0, "title".into(), NamedKind::Heading, "[1]:block".into()),
            ]
        );
        // Two tags on a line are a paragraph, and the place the start of it.
        assert_eq!(
            named("<a name=\"top\"></a>\n\n# Title\n")[0],
            (0, "top".into(), NamedKind::Html, "[0]:0@0..0".into())
        );
    }

    #[test]
    fn a_link_s_kind_is_told_by_its_address() {
        let table = [
            ("", Goes::Nowhere),
            ("#x", here("x")),
            ("#%C3%A4", here("ä")),
            ("#", here("")),
            ("b.md", file_goes("b.md", None, true)),
            ("b.md#x", file_goes("b.md", Some("x"), true)),
            ("b.md#", file_goes("b.md", None, true)),
            ("./sub/b.md?x=1#y", file_goes("./sub/b.md", Some("y"), true)),
            ("/abs/b.md", file_goes("/abs/b.md", None, true)),
            ("../B.MD", file_goes("../B.MD", None, true)),
            ("my%20file.md", file_goes("my file.md", None, true)),
            ("file:///abs/b.md", file_goes("/abs/b.md", None, true)),
            (
                "file://host/abs/b.md#x",
                file_goes("/abs/b.md", Some("x"), true),
            ),
            ("image.png", file_goes("image.png", None, false)),
            ("dir/", file_goes("dir/", None, false)),
            ("https://x", Goes::Away),
            ("HTTP://x/y.md", Goes::Away),
            ("mailto:a@b", Goes::Away),
            ("//host/x", Goes::Away),
            ("C:\\a.md", file_goes("C:\\a.md", None, true)),
        ];
        for (url, goes) in table {
            let found = of(&format!("[a](<{url}>)\n"));
            let link = found.links.first().filter(|_| !url.is_empty());
            assert_eq!(super::goes(url), goes, "{url:?}");
            if let Some(link) = link {
                assert_eq!(link.goes, goes, "{url:?} in a document");
            }
        }
    }

    #[test]
    fn a_link_has_its_title_and_its_kind() {
        let found = of("[a](#x \"t\") [b](u)\n");
        assert_eq!(found.links[0].title.as_deref(), Some("t"));
        assert_eq!(found.links[0].goes, here("x"));
        assert_eq!(found.links[1].title, None);
        assert_eq!(found.links[1].goes, file_goes("u", None, false));
    }

    #[test]
    fn named_finds_an_id_as_written_and_else_in_lower_case() {
        let found = of("# Installation\n\n<a name=\"Top\"></a>\n");
        assert_eq!(found.named(0, "installation").unwrap().id, "installation");
        assert_eq!(found.named(0, "Installation").unwrap().id, "installation");
        assert_eq!(found.named(0, "Top").unwrap().id, "Top");
        assert_eq!(found.named(0, "top"), None);
        assert_eq!(found.named(0, "other"), None);
    }
}
