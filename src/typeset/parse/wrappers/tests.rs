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
fn a_heading_with_an_id_inside_a_wrapper_keeps_the_block_as_source() {
    // The model's heading has no id, and links go to it by its id.
    let source = "<div align=\"center\">\n<h2 id=\"x\">Name</h2>\n</div>\n";
    assert_eq!(parse(source).blocks, vec![html(source)]);
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
