use std::fmt::Write;

use comrak::nodes::NodeValue;
use comrak::{Arena, parse_document};

use super::*;
use crate::typeset::parse::html::spanned_tokens;

/// The parts of the top level of `source`, as the tree takes them, and the
/// text of each HTML block.
fn pieces(source: &str) -> (Vec<Piece<'_>>, Vec<String>) {
    // The HTML blocks are owned by the arena; their text is leaked to give
    // the tokens a lifetime, in a test.
    let arena = Arena::new();
    let root = parse_document(&arena, source, &crate::typeset::parse::options());
    let (mut pieces, mut blocks) = (Vec::new(), Vec::new());
    for node in root.children() {
        match &node.data().value {
            NodeValue::HtmlBlock(block) => {
                let literal: &'static str = Box::leak(block.literal.clone().into_boxed_str());
                blocks.push(literal.to_owned());
                pieces.push(match spanned_tokens(literal) {
                    Some(tokens) => Piece::Html(tokens),
                    None => Piece::Source,
                });
            }
            _ => {
                blocks.push(String::new());
                pieces.push(Piece::Markdown);
            }
        }
    }
    (pieces, blocks)
}

/// The tree of the top level of `source`, one line for each node, indented
/// under its parent. Blanks between tags are left out.
fn tree(source: &str) -> String {
    let (pieces, blocks) = pieces(source);
    dump(&Tree::new(pieces), &blocks)
}

fn dump(tree: &Tree, blocks: &[String]) -> String {
    let mut out = String::new();
    let mut depth_of = Vec::new();
    for node in &tree.nodes {
        let depth = node.parent.map_or(0, |parent| depth_of[parent] + 1);
        depth_of.push(depth);
        let text = &blocks[node.at.piece]
            .get(node.at.bytes.clone())
            .unwrap_or("");
        let line = match &node.kind {
            NodeKind::Element(open) => {
                let how = match &open.closing {
                    Closing::End(_) => "",
                    Closing::Implied => " [implied end]",
                    Closing::Unpaired => " [unpaired start]",
                };
                format!("{}{how}", open.tag.name)
            }
            NodeKind::Text if text.trim().is_empty() => continue,
            NodeKind::Text => format!("text {:?}", text.trim()),
            NodeKind::RawTag { .. } => format!("text {:?}", text.trim()),
            NodeKind::Tag { tag, end } => {
                format!("tag <{}{}>", if *end { "/" } else { "" }, tag.name)
            }
            NodeKind::Comment => "comment".to_owned(),
            NodeKind::Markdown => "markdown".to_owned(),
            NodeKind::Source => "source".to_owned(),
            NodeKind::Unpaired => format!("unpaired {text}"),
        };
        writeln!(out, "{}{line}", "  ".repeat(depth)).unwrap();
    }
    out
}

#[test]
fn a_start_tag_ends_an_open_p_and_a_block_element_nests() {
    assert_eq!(
        tree("<p>a<div>b</div></p>\n"),
        "p [implied end]\n  text \"a\"\ndiv\n  text \"b\"\nunpaired </p>\n"
    );
    assert_eq!(
        tree("<div><p>a<p>b</div>\n"),
        "div\n  p [implied end]\n    text \"a\"\n  p [implied end]\n    text \"b\"\n"
    );
    // A raw-text element that is no block in a browser does not end it.
    for raw in ["script", "style", "textarea"] {
        assert_eq!(
            tree(&format!("<p>a<{raw}>x</{raw}>b</p>\n")),
            format!("p\n  text \"a\"\n  {raw}\n    text \"x\"\n  text \"b\"\n"),
            "{raw}"
        );
    }
    // A `<pre>` is a block, and does.
    assert_eq!(
        tree("<p>a<pre>x</pre>b\n"),
        "p [implied end]\n  text \"a\"\npre\n  text \"x\"\ntext \"b\"\n"
    );
    // So does the start of a block that kiri leaves as source, such as a
    // `<table>`, and a void one such as `<hr>`.
    assert_eq!(
        tree("<p>a<table>x</table>\n"),
        "p [implied end]\n  text \"a\"\ntable\n  text \"x\"\n"
    );
    assert_eq!(
        tree("<p>a<hr>b</p>\n"),
        "p [implied end]\n  text \"a\"\ntag <hr>\ntext \"b\"\nunpaired </p>\n"
    );
    // A text element is a leaf, and so is one the table does not know.
    assert_eq!(
        tree("<p>x <b>y</b> <video>z</video></p>\n"),
        "p\n  text \"x\"\n  tag <b>\n  text \"y\"\n  tag </b>\n  tag <video>\n  text \"z\"\n  tag </video>\n"
    );
}

#[test]
fn an_end_tag_ends_the_nearest_open_element_of_its_name() {
    assert_eq!(
        tree("<div><div>a</div>b</div>\n"),
        "div\n  div\n    text \"a\"\n  text \"b\"\n"
    );
    // Nothing of its name is open.
    assert_eq!(tree("a\n\n</div>\n"), "markdown\nunpaired </div>\n");
    assert_eq!(tree("</p>\n"), "unpaired </p>\n");
    // An element above whose end tag may be left out is whole.
    assert_eq!(
        tree("<div><p>a</div>\n"),
        "div\n  p [implied end]\n    text \"a\"\n"
    );
}

#[test]
fn a_fault_is_shown_by_the_dim_tags_both_ways() {
    // A `</div>` forgotten: the `<div>` is the one given up.
    assert_eq!(
        tree("<details><div>text</details>\n"),
        "details\n  div [unpaired start]\n    text \"text\"\n"
    );
    // A `</div>` too many: the end tag that would end the `<details>` open
    // above its `<div>`, which has its own end tag to come, is the unpaired.
    assert_eq!(
        tree("<div><details>text</div>more</details></div>\n"),
        "div\n  details\n    text \"text\"\n    unpaired </div>\n    text \"more\"\n"
    );
    // Crossed: the `<div>` pair is dim.
    assert_eq!(
        tree("<div><details>text</div></details>\n"),
        "div [unpaired start]\n  details\n    text \"text\"\n    unpaired </div>\n"
    );
    // The inner `</div>` forgotten: it cannot be told which, the outer is dim.
    assert_eq!(
        tree("<div><div>text</div>more\n"),
        "div [unpaired start]\n  div\n    text \"text\"\n  text \"more\"\n"
    );
}

#[test]
fn a_block_of_markdown_ends_an_open_p_and_is_a_leaf_of_the_element_open_last() {
    // A paragraph between `<p>` and `</p>` is not in the `<p>`: comrak
    // writes a `<p>` of its own for it, which ends the first.
    assert_eq!(
        tree("<p align=\"center\">\n\ntext\n\n</p>\n"),
        "p [implied end]\nmarkdown\nunpaired </p>\n"
    );
    assert_eq!(
        tree("<div align=\"center\">\n\ntext\n\n</div>\n"),
        "div\n  markdown\n"
    );
    assert_eq!(
        tree("<details>\n\n## h\n\n</details>\n"),
        "details\n  markdown\n"
    );
}

#[test]
fn an_element_never_ended_is_an_unpaired_start_with_its_content_still_in_it() {
    assert_eq!(
        tree("<div>a\n\ntext\n\n<center>b\n"),
        "div [unpaired start]\n  text \"a\"\n  markdown\n  center [unpaired start]\n    text \"b\"\n"
    );
    // A `<p>` may leave its end tag out.
    assert_eq!(tree("<p>a\n"), "p [implied end]\n  text \"a\"\n");
    // An unclosed `<details>` is broken as any other element.
    assert_eq!(
        tree("<details>a\n\nb\n"),
        "details [unpaired start]\n  text \"a\"\n  markdown\n"
    );
}

#[test]
fn the_end_tag_of_an_element_may_be_in_another_html_block() {
    assert_eq!(
        tree("<div>\n\ntext\n\n</div>\n\n<div>\n\nmore\n\n</div>\n"),
        "div\n  markdown\ndiv\n  markdown\n"
    );
    // Cut by a blank line, the end tag is still counted for the start tag.
    assert_eq!(
        tree("<details>\n<div>\n\ntext\n\n</div>\n</details>\n"),
        "details\n  div\n    markdown\n"
    );
}

#[test]
fn nothing_in_a_raw_text_element_is_a_tag() {
    assert_eq!(tree("<pre><div></pre>\n"), "pre\n  text \"<div>\"\n");
    // Its tags are not counted: without the `</details>` in the `<pre>` the
    // `<details>` has no end tag to come, and the `</div>` ends the `<div>`.
    assert_eq!(
        tree("<div><details>x</div><pre></details></pre>\n"),
        "div\n  details [unpaired start]\n    text \"x\"\npre\n  text \"</details>\"\n"
    );
    // One that is never ended holds the rest of the container.
    assert_eq!(
        tree("<pre>a\n<div>\n</div>\n"),
        "pre [unpaired start]\n  text \"a\"\n  text \"<div>\"\n  text \"</div>\"\n"
    );
}

/// A `<pre>` that began inside an HTML block that a blank line cut has its
/// end tag in Markdown, which the tree does not see: it is not open in the
/// next piece, and the HTML after it is read.
#[test]
fn a_raw_text_element_that_its_piece_leaves_open_does_not_run_on() {
    assert_eq!(
        tree("<div>\n<pre>\nfoo\n\nbar\n</pre>\n</div>\n\n<p>x</p>\n"),
        "div\n  pre [unpaired start]\n    text \"foo\"\n  markdown\np\n  text \"x\"\n"
    );
}

#[test]
fn comments_are_leaves_and_a_block_that_cannot_be_read_is_one() {
    assert_eq!(
        tree("<div><!-- c -->x</div>\n"),
        "div\n  comment\n  text \"x\"\n"
    );
    assert_eq!(tree("<div x=\"y\n"), "source\n");
}

/// The pieces of a script of lines: a line that starts with `<` is an HTML
/// block of its own, any other line is a block of Markdown. The tree of
/// them, without a parse of Markdown, so that its time is the tree's.
fn scripted(script: &str) -> Tree {
    let pieces = (script.lines())
        .map(|line| match line.starts_with('<') {
            true => spanned_tokens(line).map_or(Piece::Source, Piece::Html),
            false => Piece::Markdown,
        })
        .collect();
    Tree::new(pieces)
}

#[test]
fn at_most_256_block_elements_are_open_and_the_start_tags_past_that_are_unpaired() {
    let depth = |tree: &Tree| {
        let mut depths: Vec<usize> = Vec::new();
        for node in &tree.nodes {
            depths.push(node.parent.map_or(0, |parent| depths[parent] + 1));
        }
        depths.into_iter().max().unwrap_or(0)
    };
    let count = |tree: &Tree, keep: fn(&NodeKind) -> bool| {
        tree.nodes.iter().filter(|node| keep(&node.kind)).count()
    };
    let source = format!("{}{}", "<div>".repeat(300), "</div>".repeat(300));
    let tree = scripted(&source);
    assert_eq!(
        count(&tree, |kind| matches!(kind, NodeKind::Element(_))),
        256
    );
    // The unpaired start tags are leaves of the innermost element.
    assert_eq!(depth(&tree), 256);
    // The 44 start tags past the cap, and the 44 end tags that are left
    // when the 256 elements are ended.
    // An unpaired tag is a start or an end tag, as its bytes tell.
    let unpaired = |end: bool| {
        move |tree: &Tree, source: &str| {
            (tree.nodes.iter())
                .filter(|node| {
                    matches!(node.kind, NodeKind::Unpaired)
                        && source[node.at.bytes.clone()].starts_with("</") == end
                })
                .count()
        }
    };
    assert_eq!(unpaired(false)(&tree, &source), 44);
    assert_eq!(unpaired(true)(&tree, &source), 44);
    // Every element is ended by an end tag: the pairs are the 256 inner
    // elements' own, from the inside.
    assert!(tree.nodes.iter().all(|node| match &node.kind {
        NodeKind::Element(open) => matches!(open.closing, Closing::End(_)),
        _ => true,
    }));
    // Shallower, nothing is unpaired.
    let source = format!("{}{}", "<div>".repeat(256), "</div>".repeat(256));
    let tree = scripted(&source);
    assert_eq!(
        unpaired(false)(&tree, &source) + unpaired(true)(&tree, &source),
        0
    );
    // A raw-text element past the cap: its tags are unpaired leaves, what it
    // holds is still no tag, and the 256 elements below are ended by their
    // own end tags.
    let source = format!(
        "{}<pre><div></pre>{}",
        "<div>".repeat(256),
        "</div>".repeat(256)
    );
    let tree = scripted(&source);
    assert_eq!(unpaired(false)(&tree, &source), 1);
    assert_eq!(unpaired(true)(&tree, &source), 1);
    let kinds: Vec<_> = (tree.nodes.iter())
        .filter(|node| !matches!(node.kind, NodeKind::Element(_)))
        .map(|node| (&source[node.at.bytes.clone()], &node.kind))
        .collect();
    assert!(
        matches!(
            kinds.as_slice(),
            [
                ("<pre>", NodeKind::Unpaired),
                ("<div>", NodeKind::RawTag { .. }),
                ("</pre>", NodeKind::Unpaired),
            ]
        ),
        "{kinds:?}"
    );
    assert_eq!(
        count(&tree, |kind| matches!(kind, NodeKind::Element(_))),
        256
    );
    assert!(tree.nodes.iter().all(|node| match &node.kind {
        NodeKind::Element(open) => matches!(open.closing, Closing::End(_)),
        _ => true,
    }));
}

/// How the time to make the tree of `make(n)`, a script of lines, grows
/// from `n` to eight times `n`.
fn growth(make: impl Fn(usize) -> String, n: usize) -> f64 {
    crate::testing::growth(
        |script| {
            let tree = scripted(script);
            // Every tag and text is a node, and a node takes no more than 24 bytes
            // of these scripts.
            assert!(
                tree.nodes.len() >= script.len() / 24,
                "{}",
                tree.nodes.len()
            );
        },
        make,
        n,
    )
}

// The nests of the growth tests below go deeper than `MAX_DEPTH`, and past
// it every start tag is an unpaired leaf. The cap bounds any scan of the open
// elements, so a scan per tag would not make these tests fail: they guard
// against work over the whole list of nodes, such as moving nodes or reading
// tokens again.

#[test]
fn elements_one_in_another_cut_by_blank_lines_are_read_in_linear_time() {
    // Plain `<div>`, each start tag and end tag its own HTML block, with a
    // block of Markdown between.
    let plain = |n| format!("{}{}", "<div>\nm\n".repeat(n), "</div>\nm\n".repeat(n));
    let growth_plain = growth(plain, 2_000);
    assert!(growth_plain < 24.0, "plain: {growth_plain}");
    let centring = |n| {
        let (open, close) = ("<div align=\"center\">\nm\n", "</div>\nm\n");
        format!("{}{}", open.repeat(n), close.repeat(n))
    };
    let growth_centring = growth(centring, 2_000);
    assert!(growth_centring < 24.0, "centring: {growth_centring}");
}

#[test]
fn elements_never_ended_then_stray_end_tags_are_read_in_linear_time() {
    let stray = |n| format!("{}{}", "<div>\n".repeat(n), "</p>\n".repeat(n));
    let growth_stray = growth(stray, 2_000);
    assert!(growth_stray < 24.0, "stray end tags: {growth_stray}");
    // End tags of an element below others that are open, one of which has
    // its end tag to come: each end tag is the one too many, and is found
    // without a look at the elements above its own. (The cap would bound
    // such a look too; see above.)
    let blocked = |n| {
        format!(
            "<center>\n<details>\n{}{}</details>\n",
            "<div>\n".repeat(n),
            "</center>\n".repeat(n)
        )
    };
    let growth_blocked = growth(blocked, 2_000);
    assert!(growth_blocked < 24.0, "blocked: {growth_blocked}");
}

#[test]
fn many_elements_in_one_html_block_are_read_in_linear_time() {
    let growth_one_block = growth(|n| "<details>t</details>".repeat(n), 2_000);
    assert!(growth_one_block < 24.0, "{growth_one_block}");
}
