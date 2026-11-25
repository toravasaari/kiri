use crate::typeset::model::{Extent, Place};
use crate::typeset::parse::*;

fn text(text: &str) -> Inline {
    Inline::Text(text.into())
}

fn paragraph(content: &str) -> Block {
    Block::Paragraph(vec![text(content)])
}

fn heading(level: u8, content: &str) -> Block {
    Block::Heading {
        level,
        content: vec![text(content)],
        id: None,
    }
}

fn link(url: &str, content: &str) -> Inline {
    Inline::Link {
        url: url.into(),
        title: None,
        content: vec![text(content)],
    }
}

fn centred(blocks: Vec<Block>) -> Block {
    Block::Centered(blocks)
}

fn html(source: &str) -> Block {
    Block::Html(source.into())
}

fn place(document: &Document, path: &[usize]) -> Place {
    document.places.get(path).expect("a place").clone()
}

// One HTML block: the whole wrapper, with no blank line in it.

#[test]
fn a_div_of_a_heading_a_paragraph_and_links_is_one_centred_block() {
    let source = "<div align=\"center\">\n  <h1>Name</h1>\n  <p>A <b>short</b> line.</p>\n  \
                  <a href=\"https://x/docs\">Docs</a> &middot;\n  \
                  <a href=\"https://x/issues\">Issues</a>\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            heading(1, "Name"),
            Block::Paragraph(vec![
                text("A "),
                Inline::Strong(vec![text("short")]),
                text(" line.")
            ]),
            Block::Paragraph(vec![
                link("https://x/docs", "Docs"),
                text(" \u{b7} "),
                link("https://x/issues", "Issues"),
            ]),
        ])]
    );
}

#[test]
fn a_p_with_align_centre_holds_one_paragraph_with_images_and_breaks() {
    let source = "<p align=\"center\">\n  <img src=\"a.png\" alt=\"A\"><br>\n  Name\n</p>\n";
    let blocks = parse(source).blocks;
    let [Block::Centered(inside)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    let [Block::Paragraph(inlines)] = inside.as_slice() else {
        panic!("{inside:?}");
    };
    assert!(matches!(inlines[0], Inline::Image(_)), "{inlines:?}");
    assert_eq!(inlines[1], Inline::LineBreak);
    assert_eq!(inlines[2], text("Name"));
}

#[test]
fn center_needs_no_align_and_align_is_read_in_any_case_with_or_without_quotes() {
    for open in [
        "<center>",
        "<CENTER>",
        "<div align=center>",
        "<div ALIGN=\"CENTER\">",
        "<div align='Center'>",
        "<div class=\"x\" align=\" center \">",
    ] {
        let name = open[1..].split([' ', '>']).next().unwrap().to_lowercase();
        let source = format!("{open}Text</{name}>\n");
        assert_eq!(
            parse(&source).blocks,
            vec![centred(vec![paragraph("Text")])],
            "{source}"
        );
    }
}

#[test]
fn a_div_or_p_without_align_or_with_another_align_is_not_a_wrapper() {
    for source in [
        "<div>Text</div>\n",
        "<p>Text</p>\n",
        "<div align=\"right\">Text</div>\n",
        "<p align=\"left\">Text</p>\n",
        "<div style=\"text-align:center\">Text</div>\n",
    ] {
        let blocks = parse(source).blocks;
        assert!(
            !matches!(blocks.as_slice(), [Block::Centered(_)]),
            "{source}: {blocks:?}"
        );
    }
}

#[test]
fn a_heading_with_align_centre_alone_in_a_block_is_centred() {
    assert_eq!(
        parse("<h1 align=\"center\">Name</h1>\n").blocks,
        vec![centred(vec![heading(1, "Name")])]
    );
    assert_eq!(
        parse("<H3 ALIGN=center>The <i>name</i></H3>\n").blocks,
        vec![centred(vec![Block::Heading {
            level: 3,
            content: vec![text("The "), Inline::Emphasis(vec![text("name")])],
            id: None,
        }])]
    );
    // Without `align` it is the heading it was.
    assert_eq!(parse("<h1>Name</h1>\n").blocks, vec![heading(1, "Name")]);
    assert_eq!(
        parse("<h1 align=\"left\">Name</h1>\n").blocks,
        vec![heading(1, "Name")]
    );
}

#[test]
fn an_image_that_a_p_with_align_centre_wraps_is_a_centred_picture() {
    let blocks = parse("<p align=\"center\"><img src=\"a.png\" alt=\"A\"></p>\n").blocks;
    let [Block::Centered(inside)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert!(
        matches!(inside.as_slice(), [Block::Paragraph(inlines)] if matches!(inlines.as_slice(), [Inline::Image(_)])),
        "{inside:?}"
    );
    // A `<picture>` is not a tag a paragraph reads, but its image is shown.
    let blocks = parse(
        "<p align=\"center\"><picture><source srcset=\"b.png\"><img src=\"a.png\"></picture></p>\n",
    )
    .blocks;
    assert!(
        matches!(blocks.as_slice(), [Block::Centered(inside)]
            if matches!(inside.as_slice(), [Block::Paragraph(inlines)]
                if matches!(inlines.as_slice(), [Inline::Image(_)]))),
        "{blocks:?}"
    );
}

#[test]
fn a_linked_image_in_a_centred_p_is_a_link_round_the_image() {
    let blocks =
        parse("<p align=center>\n  <a href=\"https://x\"><img src=a.png></a>\n</p>\n").blocks;
    let [Block::Centered(inside)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert!(
        matches!(inside.as_slice(), [Block::Paragraph(inlines)]
            if matches!(inlines.as_slice(), [Inline::Link { url, .. }] if url == "https://x")),
        "{inside:?}"
    );
}

#[test]
fn a_one_block_wrapper_with_anything_else_inside_stays_source() {
    for source in [
        "<div align=\"center\">\n<table><tr><td>x</td></tr></table>\n</div>\n",
        "<div align=\"center\">\n<ul><li>x</li></ul>\n</div>\n",
        "<div align=\"center\">\n<span>x</span>\n</div>\n",
        "<div align=\"center\">\n<h1>Name</h1>\n<blockquote>q</blockquote>\n</div>\n",
        "<div align=\"center\">\n<div>inner</div>\n</div>\n",
        "<center><h1>a <foo>b</foo></h1></center>\n",
        "<h1 align=\"center\">a <span>b</span></h1>\n",
    ] {
        let blocks = parse(source).blocks;
        assert!(
            !blocks.iter().any(|b| matches!(b, Block::Centered(_))),
            "{source}: {blocks:?}"
        );
    }
    assert_eq!(
        parse("<div align=\"center\">\n<ul><li>x</li></ul>\n</div>\n").blocks,
        vec![html(
            "<div align=\"center\">\n<ul><li>x</li></ul>\n</div>\n"
        )]
    );
}

#[test]
fn an_empty_wrapper_and_a_p_inside_a_p_stay_source() {
    for source in [
        "<div align=\"center\"></div>\n",
        "<div align=\"center\">\n</div>\n",
        "<p align=\"center\"><p>x</p></p>\n",
        "<p align=\"center\"><h1>x</h1></p>\n",
    ] {
        let blocks = parse(source).blocks;
        assert!(
            !blocks.iter().any(|b| matches!(b, Block::Centered(_))),
            "{source}: {blocks:?}"
        );
    }
}

#[test]
fn text_after_the_end_tag_in_the_same_block_keeps_the_block_as_source() {
    let source = "<div align=\"center\">\nText\n</div>\nAfter the tag.\n";
    assert_eq!(parse(source).blocks, vec![html(source)]);
}

#[test]
fn markdown_inside_a_one_block_wrapper_is_not_read() {
    assert_eq!(
        parse("<p align=\"center\">[x](y) and *z*</p>\n").blocks,
        vec![centred(vec![paragraph("[x](y) and *z*")])]
    );
}

#[test]
fn the_places_of_a_one_block_wrapper_and_of_the_blocks_in_it() {
    let source = "Before.\n\n<div align=\"center\">\n<h1>Name</h1>\n\
                  <p>Tag</p>\n</div>\n\nAfter.\n";
    let document = parse(source);
    let whole = place(&document, &[1]);
    assert_eq!((whole.line, whole.extent), (3, Extent::Exact));
    assert_eq!(
        &source[whole.bytes.clone()],
        "<div align=\"center\">\n<h1>Name</h1>\n<p>Tag</p>\n</div>"
    );
    let first = place(&document, &[1, 0]);
    assert_eq!((first.line, first.extent), (4, Extent::Inside));
    assert_eq!(first.bytes, whole.bytes);
    let second = place(&document, &[1, 1]);
    assert_eq!((second.line, second.extent), (5, Extent::Inside));
    assert_eq!(place(&document, &[2]).line, 8);
}

#[test]
fn a_one_block_wrapper_is_read_in_a_quote_a_list_item_and_a_details() {
    let wrapper = centred(vec![paragraph("x")]);
    assert_eq!(
        parse("> <p align=\"center\">x</p>\n").blocks,
        vec![Block::BlockQuote(vec![wrapper.clone()])]
    );
    let blocks = parse("- <p align=\"center\">x</p>\n").blocks;
    let [Block::List(list)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert_eq!(list.items[0].blocks, vec![wrapper.clone()]);
    assert_eq!(
        parse("<details>\n<summary>S</summary>\n\n<p align=\"center\">x</p>\n\n</details>\n")
            .blocks,
        vec![Block::Details {
            summary: vec![text("S")],
            blocks: vec![wrapper],
        }]
    );
}

// Blank lines inside the wrapper: a block for each of its tags and the
// blocks between them.

#[test]
fn a_wrapper_that_blank_lines_split_is_one_centred_block() {
    for (open, close) in [
        ("<p align=\"center\">", "</p>"),
        ("<div align=\"center\">", "</div>"),
        ("<center>", "</center>"),
        ("<DIV ALIGN=center>", "</DIV>"),
    ] {
        let source = format!("{open}\n\nSome *text*.\n\n- an item\n\n{close}\n\nAfter.\n");
        assert_eq!(
            parse(&source).blocks,
            vec![
                centred(vec![
                    Block::Paragraph(vec![
                        text("Some "),
                        Inline::Emphasis(vec![text("text")]),
                        text("."),
                    ]),
                    Block::List(List {
                        start: None,
                        tight: true,
                        footnotes: false,
                        items: vec![ListItem {
                            checked: None,
                            blocks: vec![paragraph("an item")],
                        }],
                    }),
                ]),
                paragraph("After."),
            ],
            "{source}"
        );
    }
}

#[test]
fn a_split_wrapper_holds_an_image_and_a_heading_of_html_and_of_markdown() {
    let source = "<p align=\"center\">\n\n# Title\n\n<img src=\"a.png\" alt=\"A\">\n\n\
                  <h2>Sub</h2>\n\n</p>\n";
    let blocks = parse(source).blocks;
    let [Block::Centered(inside)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert_eq!(inside.len(), 3, "{inside:?}");
    assert_eq!(inside[0], heading(1, "Title"));
    assert!(
        matches!(&inside[1], Block::Paragraph(i) if matches!(i.as_slice(), [Inline::Image(_)]))
    );
    assert_eq!(inside[2], heading(2, "Sub"));
}

#[test]
fn the_places_of_a_split_wrapper_run_from_its_start_tag_to_its_end_tag() {
    let source = "Before.\n\n<div align=\"center\">\n\nIn *one*.\n\nIn two.\n\n</div>\n\nAfter.\n";
    let document = parse(source);
    assert_eq!(document.blocks.len(), 3);
    let whole = place(&document, &[1]);
    assert_eq!((whole.line, whole.extent), (3, Extent::Exact));
    assert_eq!(
        &source[whole.bytes.clone()],
        "<div align=\"center\">\n\nIn *one*.\n\nIn two.\n\n</div>"
    );
    let first = place(&document, &[1, 0]);
    assert_eq!((first.line, first.extent), (5, Extent::Exact));
    assert_eq!(&source[first.bytes.clone()], "In *one*.");
    assert_eq!(place(&document, &[1, 1]).line, 7);
    assert_eq!(place(&document, &[2]).line, 11);
}

#[test]
fn a_split_wrapper_in_a_quote_or_a_list_item_is_joined() {
    let wrapper = centred(vec![paragraph("x")]);
    assert_eq!(
        parse("> <p align=\"center\">\n>\n> x\n>\n> </p>\n").blocks,
        vec![Block::BlockQuote(vec![wrapper.clone()])]
    );
    let blocks = parse("- <center>\n\n  x\n\n  </center>\n").blocks;
    let [Block::List(list)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert_eq!(list.items[0].blocks, vec![wrapper]);
}

#[test]
fn wrappers_nest_in_each_other_and_with_details_both_ways() {
    let nested =
        "<div align=\"center\">\n\nOut.\n\n<center>\n\nIn.\n\n</center>\n\nBack.\n\n</div>\n";
    assert_eq!(
        parse(nested).blocks,
        vec![centred(vec![
            paragraph("Out."),
            centred(vec![paragraph("In.")]),
            paragraph("Back."),
        ])]
    );
    let details = |blocks| Block::Details {
        summary: vec![text("S")],
        blocks,
    };
    let in_details =
        "<details>\n<summary>S</summary>\n\n<p align=\"center\">\n\nIn.\n\n</p>\n\n</details>\n";
    assert_eq!(
        parse(in_details).blocks,
        vec![details(vec![centred(vec![paragraph("In.")])])]
    );
    let around_details = "<div align=\"center\">\n\n<details>\n<summary>S</summary>\n\nIn.\n\n</details>\n\n</div>\n";
    assert_eq!(
        parse(around_details).blocks,
        vec![centred(vec![details(vec![paragraph("In.")])])]
    );
}

#[test]
fn a_one_block_wrapper_and_a_split_one_mix() {
    let source = "<div align=\"center\">\n\n<h1 align=\"center\">Name</h1>\n\nText.\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            centred(vec![heading(1, "Name")]),
            paragraph("Text."),
        ])]
    );
}

#[test]
fn an_unclosed_wrapper_stays_source_with_the_blocks_after_it_as_they_are() {
    let source = "<p align=\"center\">\n\nText.\n\n# Head\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            html("<p align=\"center\">\n"),
            paragraph("Text."),
            heading(1, "Head")
        ]
    );
    // In a quote, the wrapper does not reach out of it.
    let quoted = "> <p align=\"center\">\n>\n> Text.\n\nOut.\n";
    assert_eq!(
        parse(quoted).blocks,
        vec![
            Block::BlockQuote(vec![html("<p align=\"center\">\n"), paragraph("Text.")]),
            paragraph("Out."),
        ]
    );
    // The places are those of the blocks as they are.
    let document = parse(source);
    assert_eq!(place(&document, &[0]).line, 1);
    assert_eq!(place(&document, &[2]).line, 5);
}

#[test]
fn an_end_tag_that_does_not_close_the_open_wrapper_stays_source() {
    // A `</div>` while a `<p>` is open, and the `</p>` that does close it.
    let source = "<p align=\"center\">\n\nA\n\n</div>\n\nB\n\n</p>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            paragraph("A"),
            html("</div>\n"),
            paragraph("B")
        ])]
    );
    // Never closed by its own tag, so unclosed.
    assert_eq!(
        parse("<p align=\"center\">\n\nA\n\n</div>\n").blocks,
        vec![
            html("<p align=\"center\">\n"),
            paragraph("A"),
            html("</div>\n")
        ]
    );
    // An end tag with no wrapper open.
    assert_eq!(
        parse("A\n\n</p>\n\nB\n").blocks,
        vec![paragraph("A"), html("</p>\n"), paragraph("B")]
    );
    // A `</details>` does not close a wrapper, nor `</p>` a `<details>`.
    let source = "<div align=\"center\">\n\nA\n\n</details>\n\nB\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            paragraph("A"),
            html("</details>\n"),
            paragraph("B")
        ])]
    );
    let source = "<details>\n<summary>S</summary>\n\nA\n\n</p>\n\n</details>\n";
    assert_eq!(
        parse(source).blocks,
        vec![Block::Details {
            summary: vec![text("S")],
            blocks: vec![paragraph("A"), html("</p>\n")],
        }]
    );
}

#[test]
fn text_after_the_end_tag_in_its_block_follows_the_wrapper_it_ends() {
    let source = "<div align=\"center\">\n\nA\n\n</div>\nAfter the tag.\n\nMore.\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            centred(vec![paragraph("A")]),
            paragraph("After the tag."),
            paragraph("More."),
        ]
    );
    // HTML after the tag stays a block of HTML.
    let source = "<div align=\"center\">\n\nA\n\n</div>\n<table><tr><td>x</td></tr></table>\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            centred(vec![paragraph("A")]),
            html("<table><tr><td>x</td></tr></table>\n"),
        ]
    );
    // The end tag of a plain div ends it, and its text follows it.
    let source = "<div align=\"center\">\n\n<div>\n\nA\n\n</div>\nafter\n\nB\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            html("<div>\n"),
            paragraph("A"),
            html("</div>"),
            paragraph("after"),
            paragraph("B"),
        ])]
    );
    // An end tag that ends no open element leaves its block as it is.
    let source = "<p align=\"center\">\n\nA\n\n</div>\nAfter.\n\n</p>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![paragraph("A"), html("</div>\nAfter.\n")])]
    );
    // The wrapper that is never closed stays source.
    let source = "<div align=\"center\">\n\nA\n\n</p>\nAfter.\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            html("<div align=\"center\">\n"),
            paragraph("A"),
            html("</p>\nAfter.\n"),
        ]
    );
}

#[test]
fn the_places_of_a_wrapper_end_at_its_end_tag_and_the_text_after_it_has_its_line() {
    let source = "<div align=\"center\">\n\nA\n\n</div>\nAfter the tag.\n";
    let document = parse(source);
    let wrapper = place(&document, &[0]);
    assert_eq!((wrapper.line, wrapper.extent), (1, Extent::Exact));
    assert_eq!(
        &source[wrapper.bytes.clone()],
        "<div align=\"center\">\n\nA\n\n</div>"
    );
    assert_eq!(place(&document, &[1]).line, 6);
}

#[test]
fn a_start_tag_with_text_in_its_block_is_not_a_wrapper_start() {
    let source = "<div align=\"center\">\nText.\n\nMore.\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            html("<div align=\"center\">\nText.\n"),
            paragraph("More."),
            html("</div>\n")
        ]
    );
}

#[test]
fn a_div_or_p_without_align_does_not_start_a_split_wrapper() {
    let source = "<div>\n\nA\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![html("<div>\n"), paragraph("A"), html("</div>\n")]
    );
}

#[test]
fn a_plain_div_inside_a_wrapper_is_closed_by_its_own_end_tag() {
    let source = "<div align=\"center\">\n\n# Title\n\n<div>\n\ninner text\n\n</div>\n\n\
                  after inner\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            heading(1, "Title"),
            html("<div>\n"),
            paragraph("inner text"),
            html("</div>\n"),
            paragraph("after inner"),
        ])]
    );
    // The same with a `<p>` inside a `<p>`.
    let source = "<p align=\"center\">\n\n<p>\n\nA\n\n</p>\n\nB\n\n</p>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            html("<p>\n"),
            paragraph("A"),
            html("</p>\n"),
            paragraph("B"),
        ])]
    );
    // The plain div ends first, so the wrapper round it is never closed.
    let source = "<div align=\"center\">\n\n<div>\n\nA\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            html("<div align=\"center\">\n"),
            html("<div>\n"),
            paragraph("A"),
            html("</div>\n"),
        ]
    );
    // A plain div that is never closed leaves everything as source.
    let source = "<div align=\"center\">\n\n<div>\n\nA\n\n</div>\n\n</div>\n\n";
    assert_eq!(parse(source).blocks.len(), 1);
    let source = "<div>\n\nA\n";
    assert_eq!(parse(source).blocks, vec![html("<div>\n"), paragraph("A")]);
}

#[test]
fn a_details_end_tag_closes_the_details_round_an_unclosed_wrapper() {
    let source = "<details><summary>S</summary>\n\n<p align=\"center\">\n\ncentred text\n\n\
                  </details>\n\nafter\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            Block::Details {
                summary: vec![text("S")],
                blocks: vec![html("<p align=\"center\">\n"), paragraph("centred text")],
            },
            paragraph("after"),
        ]
    );
    // A wrapper inside a wrapper inside the details, and a plain div.
    let source = "<details>\n<summary>S</summary>\n\n<div>\n\n<div align=\"center\">\n\n\
                  A\n\n</details>\n\nafter\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            Block::Details {
                summary: vec![text("S")],
                blocks: vec![
                    html("<div>\n"),
                    html("<div align=\"center\">\n"),
                    paragraph("A"),
                ],
            },
            paragraph("after"),
        ]
    );
}

#[test]
fn a_heading_with_an_id_in_a_wrapper_is_centred_and_keeps_its_id() {
    let source = "<div align=\"center\">\n<h2 id=\"x\">Name</h2>\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![Block::Heading {
            level: 2,
            content: vec![text("Name")],
            id: Some("x".into()),
        }])]
    );
    // An empty `id` is none.
    let source = "<div align=\"center\">\n<h2 id=\"\">Name</h2>\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![heading(2, "Name")])]
    );
}

#[test]
fn a_plain_div_whose_start_block_holds_text_is_closed_by_its_own_end_tag() {
    let source = "<div align=\"center\">\n\n<div class=\"x\">hello\n\ntext\n\n</div>\n\n\
                  after\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            html("<div class=\"x\">hello\n"),
            paragraph("text"),
            html("</div>\n"),
            paragraph("after"),
        ])]
    );
    // The same for a `<p>`, and for a start tag that centres but has text
    // in its block, which is no wrapper either.
    let source = "<div align=\"center\">\n\n<p>hello\n\ntext\n\n</p>\n\nafter\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            html("<p>hello\n"),
            paragraph("text"),
            html("</p>\n"),
            paragraph("after"),
        ])]
    );
    let source = "<div align=\"center\">\n\n<div align=\"center\">hello\n\ntext\n\n</div>\n\n\
                  after\n\n</div>\n";
    assert_eq!(parse(source).blocks.len(), 1, "{:?}", parse(source).blocks);
    // Two start tags in one block are two elements to close.
    let source = "<div align=\"center\">\n\n<div><div>hello\n\n</div>\n\n</div>\n\nafter\n\n\
                  </div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            html("<div><div>hello\n"),
            html("</div>\n"),
            html("</div>\n"),
            paragraph("after"),
        ])]
    );
    // A start tag that the same block ends is balanced, and a `<p>` that
    // an end tag of the `<div>` round it closes does not keep it open.
    let source = "<div align=\"center\">\n\n<div>a</div>\n\nA\n\n<p>open\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![
            html("<div>a</div>\n"),
            paragraph("A"),
            html("<p>open\n"),
        ])]
    );
    // Never closed: everything stays source.
    let source = "<div align=\"center\">\n\n<div class=\"x\">hello\n\ntext\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            html("<div align=\"center\">\n"),
            html("<div class=\"x\">hello\n"),
            paragraph("text"),
            html("</div>\n"),
        ]
    );
}

// An open `<p>` that does not centre ends where a browser ends it: at the
// end tag of an element round it and at the start of a `<p>`, `<div>` or
// `<center>`.

#[test]
fn an_end_tag_run_that_starts_a_block_ends_each_element_it_closes() {
    // `</p></div>`: the `</p>` ends the plain `<p>`, the `</div>` the wrapper.
    let source = "<div align=\"center\">\n\n<p>\n\ntext\n\n</p></div>\n\nafter\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            centred(vec![html("<p>\n"), paragraph("text"), html("</p>")]),
            paragraph("after"),
        ]
    );
    // `</div>` and then the start of the next wrapper in one block.
    let source = "<div align=\"center\">\n\nA\n\n</div>\n<div align=\"center\">\n\nB\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![centred(vec![paragraph("A")]), centred(vec![paragraph("B")])]
    );
    // Three end tags that end three elements.
    let source = "<div align=\"center\">\n\n<div>\n\n<p>\n\nA\n\n</p></div></div>\n\nafter\n";
    let blocks = parse(source).blocks;
    assert!(
        matches!(blocks.as_slice(), [Block::Centered(_), Block::Paragraph(_)]),
        "{blocks:?}"
    );
    // An end tag that ends nothing keeps its block as it was.
    let source = "A\n\n</p></div>\n\nB\n";
    assert_eq!(
        parse(source).blocks,
        vec![paragraph("A"), html("</p></div>\n"), paragraph("B")]
    );
}

#[test]
fn an_unclosed_p_does_not_stop_the_end_tag_of_a_wrapper_round_it() {
    for (open, close) in [
        ("<center>", "</center>"),
        ("<div align=\"center\">", "</div>"),
        ("<div>\n\n<div align=\"center\">", "</div>"),
    ] {
        let source = format!("{open}\n\n<p>\n\n# Title\n\n{close}\n\nafter\n");
        let blocks = parse(&source).blocks;
        let Some(Block::Centered(inside)) = blocks.iter().find(|b| matches!(b, Block::Centered(_)))
        else {
            panic!("{source}: {blocks:?}");
        };
        assert_eq!(
            inside.as_slice(),
            [html("<p>\n"), heading(1, "Title")],
            "{source}"
        );
        assert_eq!(blocks.last(), Some(&paragraph("after")), "{source}");
    }
}

#[test]
fn the_start_of_a_wrapper_ends_a_p_that_is_open() {
    let source = "<p>\n\nplain text\n\n<p align=\"center\">\n\ncentred text\n\n</p>\n\nafter\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            html("<p>\n"),
            paragraph("plain text"),
            centred(vec![paragraph("centred text")]),
            paragraph("after"),
        ]
    );
    // `<div>` and `<center>` end it as well.
    let source = "<p>\n\nplain text\n\n<center>\n\ncentred text\n\n</center>\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            html("<p>\n"),
            paragraph("plain text"),
            centred(vec![paragraph("centred text")]),
        ]
    );
    let source = "<p>\n\nplain text\n\n<div align=\"center\">\n\ncentred text\n\n</div>\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            html("<p>\n"),
            paragraph("plain text"),
            centred(vec![paragraph("centred text")]),
        ]
    );
}

#[test]
fn a_wrapper_that_starts_after_an_end_tag_in_its_block_starts_on_its_own_line() {
    let source = "<div align=\"center\">\n\nA\n\n</div>\n<div align=\"center\">\n\nB\n\n</div>\n";
    let document = parse(source);
    let second = place(&document, &[1]);
    assert_eq!((second.line, second.extent), (6, Extent::Exact));
    assert_eq!(
        &source[second.bytes.clone()],
        "<div align=\"center\">\n\nB\n\n</div>"
    );
    // The same in a quote, where the lines start after `> `.
    let source = "> <div align=\"center\">\n>\n> A\n>\n> </div>\n> <div align=\"center\">\n>\n> B\n>\n> </div>\n";
    let document = parse(source);
    let second = place(&document, &[0, 1]);
    assert_eq!(second.line, 6);
    assert!(
        source[second.bytes.clone()].starts_with("<div align=\"center\">"),
        "{:?}",
        &source[second.bytes.clone()]
    );
}
