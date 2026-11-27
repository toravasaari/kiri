use super::*;
use crate::typeset::model::{Block, Inline};
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
        ids("<details>\n<summary>S</summary>\n\n<div>\n<h2>Inside</h2>\n</div>\n\n</details>\n"),
        [heading("inside")]
    );
    // A tag with no end, and a heading in a heading, are none.
    assert_eq!(ids("<div>\n<h2>Open\n</div>\n"), []);
}

#[test]
fn a_link_finds_a_heading_inside_html_by_its_id() {
    let found = of("<div align=\"center\">\n<h2 id=\"top\">Top</h2>\n<h2>In div</h2>\n</div>\n");
    assert_eq!(found.named(0, "top").unwrap().kind, NamedKind::Heading);
    assert!(found.named(0, "in-div").is_some());
    assert_eq!(found.named(0, "In-Div").unwrap().id, "in-div");
}

#[test]
fn an_a_id_and_an_a_name_in_text_and_alone_are_named_places() {
    assert_eq!(
        named("text <a id=\"x\"></a> more <a name=\"y\">z</a>\n"),
        [
            (0, "x".into(), NamedKind::Anchor, "[0]:0@5..5".into()),
            (0, "y".into(), NamedKind::Anchor, "[0]:0@11..11".into()),
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
        (0, "top".into(), NamedKind::Anchor, "[0]:0@0..0".into())
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

#[test]
fn a_footnote_and_its_reference_are_named_places_that_their_links_go_to() {
    let found = of("A[^1] and B[^1].\n\n[^1]: Note.\n");
    let ids: Vec<_> = found
        .named
        .iter()
        .map(|n| (n.id.as_str(), n.kind))
        .collect();
    assert_eq!(
        ids,
        [
            ("fnref-1", NamedKind::Anchor),
            ("fnref-1-2", NamedKind::Anchor),
            ("fn-1", NamedKind::Anchor),
        ]
    );
    let goes: Vec<_> = found.links.iter().map(|link| link.goes.clone()).collect();
    assert_eq!(
        goes,
        [
            here("fn-1"),
            here("fn-1"),
            here("fnref-1"),
            here("fnref-1-2")
        ]
    );
    for link in &found.links {
        let Goes::Here { id } = &link.goes else {
            unreachable!()
        };
        assert!(found.named(0, id).is_some(), "{id} is a place");
    }
}

#[test]
fn the_number_of_a_footnote_reference_in_a_heading_is_in_its_id_as_on_github() {
    let found = of("# Head[^1]\n\n[^1]: Note.\n");
    assert_eq!(found.named[0].id, "head1");
    assert_eq!(found.named[0].kind, NamedKind::Heading);
}

#[test]
fn headings_and_anchors_inside_a_centred_wrapper_are_named_places_and_a_link_goes_to_them() {
    use crate::typeset::model::{Block, Document, Inline};
    let text = |text: &str| vec![Inline::Text(text.into())];
    let document = Document::new(vec![
        Block::Centered(vec![
            Block::Html("<a name=\"top\"></a>\n".into()),
            Block::Heading {
                level: 1,
                content: text("Name"),
                id: None,
            },
            Block::Html("<h2 align=\"center\">Sub</h2>\n".into()),
            Block::Paragraph(vec![Inline::Link {
                url: "#top".into(),
                title: None,
                content: text("up"),
            }]),
        ]),
        Block::Paragraph(text("after")),
    ]);
    let found = targets([&document]);
    let named: Vec<_> = (found.named.iter())
        .map(|named| {
            (
                named.id.as_str(),
                named.range.start.path.clone(),
                named.kind,
            )
        })
        .collect();
    assert_eq!(
        named,
        [
            ("top", vec![0, 0], NamedKind::Html),
            ("name", vec![0, 1], NamedKind::Heading),
            ("sub", vec![0, 2], NamedKind::Heading),
        ]
    );
    assert_eq!(found.links.len(), 1);
    assert_eq!(found.links[0].goes, here("top"));
    assert_eq!(found.links[0].range.start.path, [0, 3]);
    let top = found.named(0, "top").expect("the anchor is a place");
    assert_eq!(top.range.start.path, [0, 0]);
}

#[test]
fn a_parsed_centred_wrapper_has_its_headings_and_anchors_as_named_places() {
    let named = |found: &Targets| {
        (found.named.iter())
            .map(|named| (named.id.clone(), named.range.start.path.clone(), named.kind))
            .collect::<Vec<_>>()
    };
    // Set apart by blank lines: each block inside has a path of its own.
    let found =
        of("<div align=\"center\">\n\n<a name=\"top\"></a>\n\n# Name\n\n</div>\n\n[up](#top)\n");
    assert_eq!(
        named(&found),
        [
            ("top".to_owned(), vec![0, 0], NamedKind::Anchor),
            ("name".to_owned(), vec![0, 1], NamedKind::Heading),
        ]
    );
    assert_eq!(found.links[0].goes, here("top"));
    // In one HTML block: the anchor is in a paragraph, the heading is a block.
    let found = of("<div align=\"center\">\n<a name=\"top\"></a>\n<h2>Sub</h2>\n</div>\n");
    assert_eq!(
        named(&found),
        [
            ("top".to_owned(), vec![0, 0], NamedKind::Anchor),
            ("sub".to_owned(), vec![0, 1], NamedKind::Heading),
        ]
    );
}

/// The ids of the headings of `source` and the paths of the places they name.
fn heading_places(source: &str) -> Vec<(String, Vec<usize>)> {
    (of(source).named.iter())
        .filter(|named| named.kind == NamedKind::Heading)
        .map(|named| (named.id.clone(), named.range.start.path.clone()))
        .collect()
}

#[test]
fn a_heading_in_html_is_a_place_by_its_id_when_it_stands_alone() {
    let source = "<h1 id=\"top\">Name</h1>\n\n[go](#top)\n";
    assert_eq!(heading_places(source), [("top".to_owned(), vec![0])]);
    assert!(of(source).named(0, "top").is_some());
    // The id is the heading's, and no id is made of its text as well.
    assert!(of(source).named(0, "name").is_none());
    assert_eq!(
        parse(source).blocks[0],
        Block::Heading {
            level: 1,
            content: vec![Inline::Text("Name".into())],
            id: Some("top".into()),
        }
    );
}

#[test]
fn a_centred_heading_with_an_id_is_a_place_alone_and_in_a_wrapper() {
    for source in [
        "<h1 align=\"center\" id=\"top\">Name</h1>\n",
        "<div align=\"center\">\n<h1 id=\"top\">Name</h1>\n</div>\n",
        "<div align=\"center\">\n\n<h1 id=\"top\">Name</h1>\n\n</div>\n",
    ] {
        let document = parse(source);
        assert_eq!(
            document.blocks,
            [Block::Centered(vec![Block::Heading {
                level: 1,
                content: vec![Inline::Text("Name".into())],
                id: Some("top".into()),
            }])],
            "{source:?}"
        );
        assert_eq!(
            heading_places(source),
            [("top".to_owned(), vec![0, 0])],
            "{source:?}"
        );
        assert!(targets([&document]).named(0, "top").is_some());
    }
}

#[test]
fn markdown_headings_are_named_by_their_text_as_before() {
    assert_eq!(
        heading_places("# A\n\n# B\n\n## A\n"),
        [
            ("a".to_owned(), vec![0]),
            ("b".to_owned(), vec![1]),
            ("a-1".to_owned(), vec![2]),
        ]
    );
}

#[test]
fn an_id_does_not_take_a_number_of_the_ids_made_of_text() {
    let source = "<h1 id=\"x\">Same</h1>\n\n# Same\n\n<h2 id=\"y\">Same</h2>\n\n# Same\n";
    let ids: Vec<_> = heading_places(source).into_iter().map(|p| p.0).collect();
    assert_eq!(ids, ["x", "same", "y", "same-1"]);
}

#[test]
fn a_link_of_html_keeps_the_named_place_of_its_id_and_name_round_text_or_an_image() {
    for (source, id) in [
        (
            "<a id=\"sec\" href=\"https://e.com\">site</a>\n\n[back](#sec)\n",
            "sec",
        ),
        (
            "text <a id=\"sec\" href=\"https://e.com\">site</a> more\n",
            "sec",
        ),
        ("<a name=\"x\" href=\"https://e.com\">site</a>\n", "x"),
        (
            "<a id=\"sec\" href=\"https://e.com\"><img src=\"a.png\" alt=\"p\"></a>\n\n[back](#sec)\n",
            "sec",
        ),
        (
            "text <a id=\"sec\" href=\"https://e.com\"><img src=\"a.png\"></a> more\n",
            "sec",
        ),
        (
            "<p align=\"center\"><a id=\"sec\" href=\"https://e.com\"><img src=\"a.png\"></a></p>\n",
            "sec",
        ),
        (
            "<a id=\"sec\" href=\"https://e.com\">\n<img src=\"a.png\">\n</a>\n",
            "sec",
        ),
    ] {
        let found = of(source);
        assert!(
            found.named(0, id).is_some(),
            "{source:?}: {:?}",
            found.named
        );
        assert!(
            found.links.iter().any(|link| link.url == "https://e.com"),
            "{source:?}"
        );
    }
    let found = of("<a id=\"a\" name=\"b\" href=\"https://e.com\">site</a>\n");
    let ids: Vec<_> = found.named.iter().map(|named| named.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
}

#[test]
fn an_anchor_of_html_that_its_end_tag_closes_is_a_named_place_and_no_tags() {
    for (source, id, words) in [
        ("<a name=\"top\"></a>\n\n[up](#top)\n", "top", ""),
        ("Go <a name=\"x\">here</a> now.\n", "x", "Go here now."),
        ("<a id=\"y\"></a>Title\n", "y", "Title"),
    ] {
        let found = of(source);
        assert!(
            found.named(0, id).is_some(),
            "{source:?}: {:?}",
            found.named
        );
        let document = parse(source);
        let Block::Paragraph(content) = &document.blocks[0] else {
            panic!("{source:?}: {:?}", document.blocks);
        };
        assert!(
            !content
                .iter()
                .any(|inline| matches!(inline, Inline::Html(_))),
            "{source:?}: {content:?}"
        );
        assert_eq!(Inline::plain_text(content), words, "{source:?}");
    }
    // A start tag that no end tag closes stays as it is written.
    let document = parse("<a name=\"z\">open\n");
    let Block::Paragraph(content) = &document.blocks[0] else {
        panic!("{:?}", document.blocks);
    };
    assert!(matches!(content[0], Inline::Html(_)), "{content:?}");
}
