use crate::typeset::model::Length;
use crate::typeset::parse::*;

/// An image of `url` with `alt` text and no size.
fn image(url: &str, alt: Vec<Inline>) -> Inline {
    Inline::Image(Image {
        url: url.into(),
        alt,
        width: None,
        height: None,
    })
}

fn text(text: &str) -> Inline {
    Inline::Text(text.into())
}

#[test]
fn parses_headings_and_inline_styles() {
    let document = parse("# Title\n\nSome *emphasised*\n`code`.\n");
    assert_eq!(
        document.blocks,
        vec![
            Block::Heading {
                level: 1,
                content: vec![text("Title")],
            },
            Block::Paragraph(vec![
                text("Some "),
                Inline::Emphasis(vec![text("emphasised")]),
                Inline::SoftBreak,
                Inline::Code("code".into()),
                text("."),
            ]),
        ]
    );
}

#[test]
fn front_matter_comes_before_the_markdown_after_it() {
    let document = parse("---\ntitle: A\n---\n# Body\n");
    assert_eq!(
        document.blocks,
        vec![
            Block::FrontMatter(crate::typeset::model::FrontMatter {
                language: "yaml".into(),
                source: "title: A\n".into(),
                value: Some(Ok(crate::typeset::model::Value::Map(vec![(
                    "title".into(),
                    crate::typeset::model::Value::String("A".into())
                )]))),
            }),
            Block::Heading {
                level: 1,
                content: vec![text("Body")],
            },
        ]
    );
}

#[test]
fn front_matter_off_leaves_the_text_to_markdown() {
    let source = "---\ntitle: A\n---\n# Body\n";
    let document = parse_with(source, false);
    assert_eq!(
        document.blocks,
        vec![
            Block::ThematicBreak,
            Block::Heading {
                level: 2,
                content: vec![text("title: A")],
            },
            Block::Heading {
                level: 1,
                content: vec![text("Body")],
            },
        ]
    );
    assert_eq!(parse_with(source, true), parse(source));
}

#[test]
fn parses_task_list_items() {
    let document = parse("- [x] done\n- [ ] open\n");
    let Block::List(list) = &document.blocks[0] else {
        panic!("expected a list, got {:?}", document.blocks);
    };
    assert_eq!(list.start, None);
    let checked: Vec<_> = list.items.iter().map(|item| item.checked).collect();
    assert_eq!(checked, vec![Some(true), Some(false)]);
}

#[test]
fn parses_tables_with_alignment() {
    let document = parse("| a | b |\n|:--|--:|\n| 1 | 2 |\n");
    assert_eq!(
        document.blocks,
        vec![Block::Table(Table {
            alignments: vec![Alignment::Left, Alignment::Right],
            header: vec![vec![text("a")], vec![text("b")]],
            rows: vec![vec![vec![text("1")], vec![text("2")]]],
        })]
    );
}

#[test]
fn reads_an_html_img_as_an_image() {
    let expected = || image("a.png", vec![text("An image")]);
    assert_eq!(
        parse("<img src=\"a.png\" alt=\"An image\">\n").blocks,
        vec![Block::Paragraph(vec![expected()])]
    );
    assert_eq!(
        parse("<p align=\"center\">\n  <img src=\"a.png\" alt=\"An image\">\n</p>\n").blocks,
        vec![Block::Paragraph(vec![expected()])]
    );
    assert_eq!(
        parse("Text <img src=\"a.png\" alt=\"An image\"> here.\n").blocks,
        vec![Block::Paragraph(vec![
            text("Text "),
            expected(),
            text(" here.")
        ])]
    );
}

#[test]
fn an_img_without_alt_has_no_alt_text() {
    assert_eq!(
        parse("<img src=a.png>\n").blocks,
        vec![Block::Paragraph(vec![image("a.png", Vec::new())])]
    );
}

#[test]
fn reads_an_html_link_around_an_img_as_a_link() {
    let linked = || Inline::Link {
        url: "https://x".into(),
        content: vec![image("a.png", vec![text("x")])],
    };
    assert_eq!(
        parse("<a href=\"https://x\"><img src=\"a.png\" alt=\"x\"></a>\n").blocks,
        vec![Block::Paragraph(vec![linked()])]
    );
    assert_eq!(
        parse("<p>\n<a href=\"https://x\"><img src=\"a.png\" alt=\"x\"></a>\n</p>\n").blocks,
        vec![Block::Paragraph(vec![linked()])]
    );
    assert_eq!(
        parse("See <a href=\"https://x\"><img src=\"a.png\" alt=\"x\"></a>.\n").blocks,
        vec![Block::Paragraph(vec![text("See "), linked(), text(".")])]
    );
}

#[test]
fn keeps_an_html_link_around_more_than_an_img_as_source() {
    assert_eq!(
        parse("<a href=x>text <img src=a.png></a>\n").blocks,
        vec![Block::Paragraph(vec![
            Inline::Html("<a href=x>".into()),
            text("text "),
            image("a.png", Vec::new()),
            Inline::Html("</a>".into()),
        ])]
    );
}

#[test]
fn an_img_keeps_its_width_and_height() {
    assert_eq!(
        parse("<img src=a.png width=\"50%\" height=\"40\">\n").blocks,
        vec![Block::Paragraph(vec![Inline::Image(Image {
            url: "a.png".into(),
            alt: Vec::new(),
            width: Some(Length::Percent(50)),
            height: Some(Length::Pixels(40)),
        })])]
    );
}

#[test]
fn keeps_other_html_as_source() {
    assert_eq!(
        parse("<p>Text <img src=a.png></p>\n").blocks,
        vec![Block::Html("<p>Text <img src=a.png></p>\n".into())]
    );
    assert_eq!(
        parse("Text <span>bold</span>\n").blocks,
        vec![Block::Paragraph(vec![
            text("Text "),
            Inline::Html("<span>".into()),
            text("bold"),
            Inline::Html("</span>".into()),
        ])]
    );
}

#[test]
fn reads_an_html_br_as_a_line_break() {
    assert_eq!(
        parse("One<br>two<br/>three<BR />four\n").blocks,
        vec![Block::Paragraph(vec![
            text("One"),
            Inline::LineBreak,
            text("two"),
            Inline::LineBreak,
            text("three"),
            Inline::LineBreak,
            text("four"),
        ])]
    );
}

#[test]
fn reads_an_html_kbd_as_keyboard_input() {
    assert_eq!(
        parse("Press <kbd>Ctrl</kbd>+<KBD>C</KBD>.\n").blocks,
        vec![Block::Paragraph(vec![
            text("Press "),
            Inline::Kbd(vec![text("Ctrl")]),
            text("+"),
            Inline::Kbd(vec![text("C")]),
            text("."),
        ])]
    );
    assert_eq!(
        parse("*<kbd>Page **Down**</kbd>*\n").blocks,
        vec![Block::Paragraph(vec![Inline::Emphasis(vec![Inline::Kbd(
            vec![text("Page "), Inline::Strong(vec![text("Down")])]
        )])])]
    );
    assert_eq!(
        parse("<kbd><kbd>Ctrl</kbd>+<kbd>C</kbd></kbd>\n").blocks,
        vec![Block::Paragraph(vec![Inline::Kbd(vec![
            Inline::Kbd(vec![text("Ctrl")]),
            text("+"),
            Inline::Kbd(vec![text("C")]),
        ])])]
    );
}

#[test]
fn keeps_a_kbd_without_both_tags_as_source() {
    let html = |tag: &str| Inline::Html(tag.into());
    assert_eq!(
        parse("<kbd>open and </kbd>x</kbd>\n").blocks,
        vec![Block::Paragraph(vec![
            Inline::Kbd(vec![text("open and ")]),
            text("x"),
            html("</kbd>"),
        ])]
    );
    assert_eq!(
        parse("A <kbd>key *and* more\n").blocks,
        vec![Block::Paragraph(vec![
            text("A "),
            html("<kbd>"),
            text("key "),
            Inline::Emphasis(vec![text("and")]),
            text(" more"),
        ])]
    );
    assert_eq!(
        parse("*<kbd>a* b</kbd>\n").blocks,
        vec![Block::Paragraph(vec![
            Inline::Emphasis(vec![html("<kbd>"), text("a")]),
            text(" b"),
            html("</kbd>"),
        ])]
    );
}

/// The paragraph that `markdown` is, as its inlines.
fn inlines_of(markdown: &str) -> Vec<Inline> {
    match parse(markdown).blocks.as_slice() {
        [Block::Paragraph(inlines)] => inlines.clone(),
        blocks => panic!("not one paragraph: {blocks:?}"),
    }
}

#[test]
fn reads_html_emphasis_tags() {
    for (markup, make) in [
        ("b", Inline::Strong as fn(Vec<Inline>) -> Inline),
        ("strong", Inline::Strong),
        ("i", Inline::Emphasis),
        ("em", Inline::Emphasis),
        ("s", Inline::Strikethrough),
        ("del", Inline::Strikethrough),
    ] {
        let upper = markup.to_uppercase();
        for tag in [markup, upper.as_str()] {
            assert_eq!(
                inlines_of(&format!("a <{tag}>x y</{tag}> b\n")),
                vec![text("a "), make(vec![text("x y")]), text(" b")],
                "{tag}"
            );
        }
    }
    assert_eq!(
        inlines_of("a <code>x</code> b\n"),
        vec![text("a "), Inline::Code("x".into()), text(" b")]
    );
    assert_eq!(
        inlines_of("<CODE>x</CODE>\n"),
        vec![Inline::Code("x".into())]
    );
}

#[test]
fn reads_html_emphasis_tags_in_context() {
    assert_eq!(
        inlines_of("<b><i>x</i></b>\n"),
        vec![Inline::Strong(vec![Inline::Emphasis(vec![text("x")])])]
    );
    assert_eq!(
        inlines_of("*a <b>x</b>*\n"),
        vec![Inline::Emphasis(vec![
            text("a "),
            Inline::Strong(vec![text("x")])
        ])]
    );
    assert_eq!(
        inlines_of("<b>a *b* `c`</b>\n"),
        vec![Inline::Strong(vec![
            text("a "),
            Inline::Emphasis(vec![text("b")]),
            text(" "),
            Inline::Code("c".into()),
        ])]
    );
    assert_eq!(
        parse("# a <b>x</b>\n").blocks,
        vec![Block::Heading {
            level: 1,
            content: vec![text("a "), Inline::Strong(vec![text("x")])],
        }]
    );
    assert_eq!(
        parse("| <b>h</b> |\n|--|\n| <i>c</i> |\n").blocks,
        vec![Block::Table(Table {
            alignments: vec![Alignment::Left],
            header: vec![vec![Inline::Strong(vec![text("h")])]],
            rows: vec![vec![vec![Inline::Emphasis(vec![text("c")])]]],
        })]
    );
    assert_eq!(
        inlines_of("[a <del>b</del>](https://e.com)\n"),
        vec![Inline::Link {
            url: "https://e.com".into(),
            content: vec![text("a "), Inline::Strikethrough(vec![text("b")])],
        }]
    );
}

#[test]
fn keeps_html_emphasis_tags_without_both_tags_as_source() {
    let html = |tag: &str| Inline::Html(tag.into());
    assert_eq!(
        inlines_of("a <b>x\n"),
        vec![text("a "), html("<b>"), text("x")]
    );
    assert_eq!(inlines_of("x</i>\n"), vec![text("x"), html("</i>")]);
    // One tag name per element: `</strong>` does not close a `<b>`.
    assert_eq!(
        inlines_of("<b>x</strong>\n"),
        vec![html("<b>"), text("x"), html("</strong>")]
    );
    // The closing tag is matched by name, so wrong nesting still reads.
    assert_eq!(
        inlines_of("<b><i>x</b></i>\n"),
        vec![Inline::Strong(vec![html("<i>"), text("x")]), html("</i>")]
    );
}

#[test]
fn reads_an_html_code_only_around_plain_text() {
    let html = |tag: &str| Inline::Html(tag.into());
    assert_eq!(
        inlines_of("<code>a  b</code>\n"),
        vec![Inline::Code("a  b".into())]
    );
    assert_eq!(
        inlines_of("<code><b>x</b></code>\n"),
        vec![
            html("<code>"),
            Inline::Strong(vec![text("x")]),
            html("</code>")
        ]
    );
    assert_eq!(
        inlines_of("<code></code>\n"),
        vec![html("<code>"), html("</code>")]
    );
    assert_eq!(
        inlines_of("<code>a\nb</code>\n"),
        vec![
            html("<code>"),
            text("a"),
            Inline::SoftBreak,
            text("b"),
            html("</code>")
        ]
    );
    assert_eq!(
        inlines_of("<code>a*b*c</code>\n"),
        vec![
            html("<code>"),
            text("a"),
            Inline::Emphasis(vec![text("b")]),
            text("c"),
            html("</code>")
        ]
    );
    assert_eq!(
        inlines_of("<code>Vec<T></code>\n"),
        vec![html("<code>"), text("Vec"), html("<T>"), html("</code>")]
    );
}

#[test]
fn reads_html_emphasis_tags_in_a_summary() {
    let document = parse(
        "<details><summary>A <b>bold</b> and <code>code</code></summary>\n\nBody\n\n</details>\n",
    );
    assert_eq!(
        document.blocks,
        vec![Block::Details {
            summary: vec![
                text("A "),
                Inline::Strong(vec![text("bold")]),
                text(" and "),
                Inline::Code("code".into()),
            ],
            blocks: vec![Block::Paragraph(vec![text("Body")])],
        }]
    );
}

/// A heading block of `level` with `content`.
fn heading(level: u8, content: Vec<Inline>) -> Block {
    Block::Heading { level, content }
}

#[test]
fn reads_an_html_heading_as_a_heading() {
    for level in 1..=6u8 {
        assert_eq!(
            parse(&format!("<h{level}>Title</h{level}>\n")).blocks,
            vec![heading(level, vec![text("Title")])]
        );
    }
    assert_eq!(
        parse("<H1>Title</H1>\n").blocks,
        vec![heading(1, vec![text("Title")])]
    );
    assert_eq!(
        parse("<h3 align=\"center\" id=\"x\" class=y>T</h3>\n").blocks,
        vec![heading(3, vec![text("T")])]
    );
    // A comment around it counts only on the same line: comrak makes one
    // that starts a line a block of its own.
    for html in [
        "<!-- a --><h2>Title</h2><!-- b -->\n",
        "<h2>\n  Title\n</h2><!-- b -->\n",
    ] {
        assert_eq!(
            parse(html).blocks,
            vec![heading(2, vec![text("Title")])],
            "{html:?}"
        );
    }
}

#[test]
fn an_html_heading_is_the_same_as_a_markdown_one() {
    assert_eq!(
        parse("<h2>The <code>kiri</code> &mdash; <i>tool</i></h2>\n").blocks,
        parse("## The `kiri` \u{2014} *tool*\n").blocks
    );
    assert_eq!(parse("<h1>Title</h1>\n").blocks, parse("# Title\n").blocks);
}

#[test]
fn reads_inline_tags_and_entities_in_an_html_heading() {
    assert_eq!(
        parse("<h2>The <code>kiri</code> &mdash; <i>tool</i></h2>\n").blocks,
        vec![heading(
            2,
            vec![
                text("The "),
                Inline::Code("kiri".into()),
                text(" \u{2014} "),
                Inline::Emphasis(vec![text("tool")]),
            ]
        )]
    );
}

#[test]
fn an_html_heading_may_hold_a_linked_image_and_a_line_break() {
    assert_eq!(
        parse("<h1 align=\"center\"><a href=\"https://e.com\"><img src=a.png alt=A></a><br>Name</h1>\n")
            .blocks,
        vec![heading(
            1,
            vec![
                Inline::Link {
                    url: "https://e.com".into(),
                    content: vec![image("a.png", vec![text("A")])],
                },
                Inline::LineBreak,
                text("Name"),
            ]
        )]
    );
}

#[test]
fn keeps_other_html_around_a_heading_as_source() {
    for html in [
        "<h2>T\n",
        "<h2>T</h3>\n",
        "<h2></h2>\n",
        "<h2>a</h2>\n<h2>b</h2>\n",
        "<h2>a</h2>\ntext after\n",
        "<h2>a</h2>\n<div>b</div>\n",
        "<div><h2>a</h2></div>\n",
        "<h2 title=\"open>a</h2>\n",
    ] {
        match parse(html).blocks.as_slice() {
            [Block::Html(source)] => assert_eq!(source, html),
            blocks => panic!("{html:?}: {blocks:?}"),
        }
    }
    // `<h7>` starts no HTML block for comrak: its tags are inline HTML.
    assert_eq!(
        parse("<h7>T</h7>\n").blocks,
        vec![Block::Paragraph(vec![
            Inline::Html("<h7>".into()),
            text("T"),
            Inline::Html("</h7>".into()),
        ])]
    );
}

#[test]
fn an_html_heading_after_a_paragraph_is_a_block_of_its_own() {
    assert_eq!(
        parse("text before\n<h2>a</h2>\n").blocks,
        vec![
            Block::Paragraph(vec![text("text before")]),
            heading(2, vec![text("a")]),
        ]
    );
}

#[test]
fn reads_an_html_heading_in_a_list_item_and_in_details() {
    let document = parse("- item\n\n  <h2>Title</h2>\n");
    let [Block::List(list)] = document.blocks.as_slice() else {
        panic!("{:?}", document.blocks);
    };
    assert_eq!(
        list.items[0].blocks,
        vec![
            Block::Paragraph(vec![text("item")]),
            heading(2, vec![text("Title")]),
        ]
    );
    assert_eq!(
        parse("<details>\n<summary>S</summary>\n\n<h2>T</h2>\n\n</details>\n").blocks,
        vec![Block::Details {
            summary: vec![text("S")],
            blocks: vec![heading(2, vec![text("T")])],
        }]
    );
}

#[test]
fn an_html_pre_is_the_code_block_a_fence_with_its_text_is() {
    for (html, fence) in [
        (
            "<pre><code class=\"language-rust\">fn main() {}\n</code></pre>\n",
            "```rust\nfn main() {}\n```\n",
        ),
        (
            "<pre lang=\"rust\">\nfn main() {}\n</pre>\n",
            "```rust\nfn main() {}\n```\n",
        ),
        (
            "<pre lang=rust><code class=\"language-c\">x\n</code></pre>\n",
            "```rust\nx\n```\n",
        ),
        (
            "<pre><code class=\"lang-py\">x = 1\n</code></pre>\n",
            "```py\nx = 1\n```\n",
        ),
        (
            "<pre><code class=\"highlight language-sh\">ls\n</code></pre>\n",
            "```sh\nls\n```\n",
        ),
        ("<pre>\nplain\n</pre>\n", "```\nplain\n```\n"),
        ("<pre>plain</pre>\n", "```\nplain\n```\n"),
        ("<pre><code>x</code></pre>\n", "```\nx\n```\n"),
        ("<pre>\n\nx\n</pre>\n", "```\n\nx\n```\n"),
        ("<pre>\nx\n\n</pre>\n", "```\nx\n\n```\n"),
        (
            "<pre><code>&lt;pre&gt; &amp; &quot;x&quot; &mdash;\n</code></pre>\n",
            "```\n<pre> & \"x\" \u{2014}\n```\n",
        ),
        (
            "<pre><code>a\n\n    b\n\tc\n  d  \n</code></pre>\n",
            "```\na\n\n    b\n\tc\n  d  \n```\n",
        ),
        (
            "<pre>\n<code class=\"language-c\">int x;\n</code>\n</pre>\n",
            "```c\nint x;\n```\n",
        ),
        ("<PRE LANG=\"c\">x</PRE>\n", "```c\nx\n```\n"),
        (
            "<pre lang=\"mermaid\">a-->b</pre>\n",
            "```mermaid\na-->b\n```\n",
        ),
    ] {
        assert_eq!(parse(html).blocks, parse(fence).blocks, "{html:?}");
    }
}

#[test]
fn an_html_pre_keeps_its_line() {
    assert_eq!(
        parse("Text\n\n<pre>x</pre>\n").blocks,
        vec![
            Block::Paragraph(vec![text("Text")]),
            Block::CodeBlock {
                language: None,
                code: "x\n".into(),
                line: 3,
                whole_file: false,
            },
        ]
    );
}

#[test]
fn keeps_other_html_around_a_pre_as_source() {
    for html in [
        "<pre><span>x</span></pre>\n",
        "<pre><code><span>x</span></code></pre>\n",
        "<pre>x</pre> trailing text\n",
        "<pre>x</pre><b>y</b>\n",
        "<pre>x<!-- c --></pre>\n",
        "<pre>x\n",
        "<pre><code>x</pre>\n",
        "<pre></pre>\n",
        "<pre><code></code></pre>\n",
        "<pre>\n\n</pre>\n",
        "<pre> </pre>\n",
        "<pre><code>\n</code></pre>\n",
        "<pre>x</pre><pre>y</pre>\n",
        "<pre>a < b</pre>\n",
        "<pre>Vec<T></pre>\n",
        "<pre><code>x</code>y</pre>\n",
    ] {
        match parse(html).blocks.as_slice() {
            [Block::Html(source)] => assert_eq!(source, html),
            blocks => panic!("{html:?}: {blocks:?}"),
        }
    }
}

#[test]
fn reads_an_html_pre_in_a_list_item() {
    let document = parse("- item\n\n  <pre><code class=\"language-sh\">ls\n  </code></pre>\n");
    let [Block::List(list)] = document.blocks.as_slice() else {
        panic!("{:?}", document.blocks);
    };
    assert_eq!(
        list.items[0].blocks,
        vec![
            Block::Paragraph(vec![text("item")]),
            Block::CodeBlock {
                language: Some("sh".into()),
                code: "ls\n".into(),
                line: 3,
                whole_file: false,
            },
        ]
    );
}

#[test]
fn reads_an_html_mark_as_marked_text() {
    assert_eq!(
        parse("Some <mark>marked *text*</mark>, <mark><kbd>A</kbd></mark>\n").blocks,
        vec![Block::Paragraph(vec![
            text("Some "),
            Inline::Mark(vec![text("marked "), Inline::Emphasis(vec![text("text")])]),
            text(", "),
            Inline::Mark(vec![Inline::Kbd(vec![text("A")])]),
        ])]
    );
}

#[test]
fn reads_an_html_sub_and_sup_as_scripts() {
    assert_eq!(
        parse("H<sub>2</sub>O and x<SUP>*n* + 1</SUP>\n").blocks,
        vec![Block::Paragraph(vec![
            text("H"),
            Inline::Subscript(vec![text("2")]),
            text("O and x"),
            Inline::Superscript(vec![Inline::Emphasis(vec![text("n")]), text(" + 1"),]),
        ])]
    );
    assert_eq!(
        parse("e<sup>x<sub>i</sub></sup>, a <sub>b\n").blocks,
        vec![Block::Paragraph(vec![
            text("e"),
            Inline::Superscript(vec![text("x"), Inline::Subscript(vec![text("i")])]),
            text(", a "),
            Inline::Html("<sub>".into()),
            text("b"),
        ])]
    );
}

fn details(summary: Vec<Inline>, blocks: Vec<Block>) -> Block {
    Block::Details { summary, blocks }
}

fn paragraph(content: &str) -> Block {
    Block::Paragraph(vec![text(content)])
}

#[test]
fn joins_a_details_that_blank_lines_split() {
    let source = "<details>\n<summary>Open <kbd>K</kbd><br>me</summary>\n\n\
            Some *text*.\n\n- an item\n\n</details>\n\nAfter.\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            details(
                vec![
                    text("Open "),
                    Inline::Kbd(vec![text("K")]),
                    Inline::LineBreak,
                    text("me"),
                ],
                vec![
                    Block::Paragraph(vec![
                        text("Some "),
                        Inline::Emphasis(vec![text("text")]),
                        text("."),
                    ]),
                    Block::List(List {
                        start: None,
                        tight: true,
                        items: vec![ListItem {
                            checked: None,
                            blocks: vec![paragraph("an item")],
                        }],
                    }),
                ]
            ),
            paragraph("After."),
        ]
    );
}

#[test]
fn joins_a_details_in_a_list_item_or_a_quote() {
    let listed = "- Item\n  <details>\n  <summary>S</summary>\n\n  In.\n\n  </details>\n";
    let Block::List(list) = &parse(listed).blocks[0] else {
        panic!("expected a list");
    };
    assert_eq!(
        list.items[0].blocks,
        vec![
            paragraph("Item"),
            details(vec![text("S")], vec![paragraph("In.")]),
        ]
    );
    let quoted = "> <details>\n> <summary>S</summary>\n>\n> In.\n>\n> </details>\n";
    assert_eq!(
        parse(quoted).blocks,
        vec![Block::BlockQuote(vec![details(
            vec![text("S")],
            vec![paragraph("In.")]
        )])]
    );
}

#[test]
fn joins_nested_details() {
    let source = "<details>\n<summary>Out</summary>\n\n<details>\n<summary>In</summary>\n\n\
            Deep.\n\n</details>\n\nBack.\n\n</details>\n";
    assert_eq!(
        parse(source).blocks,
        vec![details(
            vec![text("Out")],
            vec![
                details(vec![text("In")], vec![paragraph("Deep.")]),
                paragraph("Back."),
            ]
        )]
    );
}

#[test]
fn joins_nested_details_that_no_blank_line_splits() {
    let source = "<details>\n<summary>Out</summary>\n<details>\n<summary>In</summary>\n\n\
            Deep.\n\n</details>\n\nBack.\n\n</details>\n\nAfter.\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            details(
                vec![text("Out")],
                vec![
                    details(vec![text("In")], vec![paragraph("Deep.")]),
                    paragraph("Back."),
                ]
            ),
            paragraph("After."),
        ]
    );
}

#[test]
fn reads_the_html_that_shares_a_block_with_details_tags_as_text() {
    let source = "<details>\n<summary>S</summary>\n<p>\nIn a paragraph.\n\n</p>\n</details>\n\n\
            After.\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            details(vec![text("S")], vec![paragraph("In a paragraph.")]),
            paragraph("After."),
        ]
    );
    let source = "<details>\n<summary>S</summary>\n\nIn.\n\n<p>More.</p>\n</details>\n\nAfter.\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            details(vec![text("S")], vec![paragraph("In."), paragraph("More.")]),
            paragraph("After."),
        ]
    );
}

#[test]
fn ends_a_details_at_a_block_that_holds_more_than_its_end() {
    let source = "<details>\n<summary>S</summary>\n\nIn.\n\n</details>\nAfter.\n\nLast.\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            details(vec![text("S")], vec![paragraph("In.")]),
            paragraph("After."),
            paragraph("Last."),
        ]
    );
    let source = "<details>\n<summary>A</summary>\n\nIn A.\n\n</details>\n<details>\n\
            <summary>B</summary>\n\nIn B.\n\n</details>\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            details(vec![text("A")], vec![paragraph("In A.")]),
            details(vec![text("B")], vec![paragraph("In B.")]),
        ]
    );
    assert_eq!(
        parse("<details>x</details><p>y</p>\n").blocks,
        vec![
            details(vec![text("Details")], vec![paragraph("x")]),
            paragraph("y")
        ]
    );
}

#[test]
fn a_details_never_closed_runs_to_the_end_of_its_container() {
    let source = "> <details>\n> <summary>S</summary>\n> More text.\n>\n> In.\n\nOut.\n";
    assert_eq!(
        parse(source).blocks,
        vec![
            Block::BlockQuote(vec![details(
                vec![text("S")],
                vec![paragraph("More text."), paragraph("In.")]
            )]),
            paragraph("Out."),
        ]
    );
}

#[test]
fn a_details_in_one_block_holds_its_text_as_a_paragraph() {
    let source =
        "<details><summary>One</summary>Hidden &amp; <b>text</b>\non two lines</details>\n";
    assert_eq!(
        parse(source).blocks,
        vec![details(
            vec![text("One")],
            vec![paragraph("Hidden & text on two lines")]
        )]
    );
    assert_eq!(
        parse("<details><summary>Empty</summary> </details>\n").blocks,
        vec![details(vec![text("Empty")], Vec::new())]
    );
    assert_eq!(
        parse("<details><summary>Logo</summary><img src=a.png alt=x></details>\n").blocks,
        vec![details(
            vec![text("Logo")],
            vec![Block::Paragraph(vec![image("a.png", vec![text("x")])])]
        )]
    );
}

#[test]
fn a_details_without_a_summary_is_called_details() {
    assert_eq!(
        parse("<details>\n\nText.\n\n</details>\n").blocks,
        vec![details(vec![text("Details")], vec![paragraph("Text.")])]
    );
    assert_eq!(
        parse("<details>\n\n<summary>Later</summary>\n\nText.\n\n</details>\n").blocks,
        vec![details(vec![text("Later")], vec![paragraph("Text.")])]
    );
}

#[test]
fn keeps_a_details_end_without_a_start_as_source() {
    assert_eq!(
        parse("Text.\n\n</details>\n").blocks,
        vec![paragraph("Text."), Block::Html("</details>\n".into())]
    );
    assert_eq!(
        parse("</details>\n<p>y</p>\n").blocks,
        vec![Block::Html("</details>\n<p>y</p>\n".into())]
    );
}

#[test]
fn keeps_a_block_that_does_not_start_with_details_as_source() {
    assert_eq!(
        parse("<p>\n<details>\n\nText.\n").blocks,
        vec![Block::Html("<p>\n<details>\n".into()), paragraph("Text.")]
    );
}

#[test]
fn turns_emoji_shortcodes_into_emoji() {
    assert_eq!(
        parse("I :rocket: it\n").blocks,
        vec![Block::Paragraph(vec![
            text("I "),
            text("\u{1F680}"),
            text(" it")
        ])]
    );
}

#[test]
fn turns_shortcodes_inside_inlines_into_emoji() {
    assert_eq!(
        parse("[:tada:](u) **:fire:** ~~:fire:~~ ![:rocket:](a.png)\n").blocks,
        vec![Block::Paragraph(vec![
            Inline::Link {
                url: "u".into(),
                content: vec![text("\u{1F389}")],
            },
            text(" "),
            Inline::Strong(vec![text("\u{1F525}")]),
            text(" "),
            Inline::Strikethrough(vec![text("\u{1F525}")]),
            text(" "),
            image("a.png", vec![text("\u{1F680}")]),
        ])]
    );
}

#[test]
fn keeps_unknown_shortcodes_and_code_as_written() {
    assert_eq!(
        parse("A :not_a_real_code: and :octocat: `:rocket:`\n").blocks,
        vec![Block::Paragraph(vec![
            text("A :not_a_real_code: and :octocat: "),
            Inline::Code(":rocket:".into()),
        ])]
    );
    assert_eq!(
        parse("```\n:rocket:\n```\n").blocks,
        vec![Block::CodeBlock {
            language: None,
            code: ":rocket:\n".into(),
            line: 1,
            whole_file: false,
        }]
    );
}

#[test]
fn keeps_only_the_language_from_the_info_string() {
    let document = parse("```rust ignore\nfn main() {}\n```\n");
    assert_eq!(
        document.blocks,
        vec![Block::CodeBlock {
            language: Some("rust".into()),
            code: "fn main() {}\n".into(),
            line: 1,
            whole_file: false,
        }]
    );
}

/// The lines the code blocks of `source` start on.
fn code_lines(blocks: &[Block], lines: &mut Vec<usize>) {
    for block in blocks {
        match block {
            Block::CodeBlock { line, .. } => lines.push(*line),
            Block::BlockQuote(blocks) => code_lines(blocks, lines),
            Block::List(list) => {
                for item in &list.items {
                    code_lines(&item.blocks, lines);
                }
            }
            _ => {}
        }
    }
}

#[test]
fn records_the_line_each_code_block_starts_on() {
    let source = "\
---
title: x
---
# Heading

```mermaid
graph TD
```

- item

  ```
  in a list
  ```

> ~~~
> quoted
> ~~~

    indented
";
    let mut lines = Vec::new();
    code_lines(&parse(source).blocks, &mut lines);
    assert_eq!(lines, [6, 12, 16, 20]);
    // Without front matter the lines are the same.
    let mut lines = Vec::new();
    code_lines(&parse_with(source, false).blocks, &mut lines);
    assert_eq!(lines, [6, 12, 16, 20]);
}

/// The lines of the blocks at `paths`.
fn lines_of(document: &Document, paths: &[&[usize]]) -> Vec<Option<usize>> {
    paths.iter().map(|path| document.lines.get(path)).collect()
}

#[test]
fn top_level_blocks_know_their_lines() {
    let document = parse("# Title\n\nOne\ntwo\n\n---\n\n```\ncode\n```\n");
    assert_eq!(
        lines_of(&document, &[&[0], &[1], &[2], &[3]]),
        [Some(1), Some(3), Some(6), Some(8)]
    );
}

#[test]
fn nested_blocks_know_their_lines() {
    let source = "\
> Quote
>
> ```
> code
> ```

- First

  Second paragraph
- Next

> [!NOTE]
> Alert text
";
    let document = parse(source);
    assert_eq!(
        lines_of(
            &document,
            &[
                &[0],
                &[0, 0],
                &[0, 1],
                &[1],
                &[1, 0, 0],
                &[1, 0, 1],
                &[1, 1, 0],
                &[2],
                &[2, 0],
            ]
        ),
        [
            Some(1),
            Some(1),
            Some(3),
            Some(7),
            Some(7),
            Some(9),
            Some(10),
            Some(12),
            Some(13)
        ]
    );
}

#[test]
fn details_and_the_blocks_in_them_know_their_lines() {
    let source = "\
Before

<details>
<summary>Outer</summary>

In outer

<details>
<summary>Inner</summary>

Deep

</details>

</details>

After
";
    let document = parse(source);
    assert_eq!(
        lines_of(
            &document,
            &[&[0], &[1], &[1, 0], &[1, 1], &[1, 1, 0], &[2], &[1, 2]]
        ),
        [Some(1), Some(3), Some(6), Some(8), Some(11), Some(17), None]
    );
}

#[test]
fn details_in_one_html_block_start_on_the_line_of_that_block() {
    let document = parse("Before\n\n<details><details>x</details></details>\n");
    assert_eq!(
        lines_of(&document, &[&[1], &[1, 0], &[1, 0, 0]]),
        [Some(3), Some(3), Some(3)]
    );
    // An image in the body is read from the HTML block it is in too.
    let source = "Before\n\n<details>\n<summary>S</summary>\n<img src=\"a.png\">\n</details>\n";
    let document = parse(source);
    assert_eq!(lines_of(&document, &[&[1], &[1, 0]]), [Some(3), Some(3)]);
    assert!(matches!(
        document.blocks[1],
        Block::Details { ref blocks, .. } if matches!(blocks[0], Block::Paragraph(_))
    ));
}

#[test]
fn lines_count_from_the_start_of_the_file_after_front_matter() {
    let document = parse("---\ntitle: x\n---\n# Heading\n\n- item\n\n  more\n");
    // The front matter is the first block, so the others move down one.
    assert_eq!(
        lines_of(&document, &[&[0], &[1], &[2], &[2, 0, 0], &[2, 0, 1]]),
        [Some(1), Some(4), Some(6), Some(6), Some(8)]
    );
    // Without front matter, the same text starts at the top.
    let document = parse_with("---\ntitle: x\n---\n# Heading\n", false);
    assert_eq!(document.lines.get(&[0]), Some(1));
}

#[test]
fn the_lines_do_not_take_part_in_comparing_documents() {
    let document = parse("one\n\n\n\ntwo\n");
    let by_hand = Document::new(vec![
        Block::Paragraph(vec![text("one")]),
        Block::Paragraph(vec![text("two")]),
    ]);
    assert_eq!(document, by_hand);
    assert_eq!(by_hand.lines.get(&[1]), None);
}

/// A formula block of `code`, which starts on `line`.
fn formula(code: &str, line: usize) -> Block {
    Block::CodeBlock {
        language: Some("math".into()),
        code: code.into(),
        line,
        whole_file: false,
    }
}

#[test]
fn reads_dollar_display_math_as_a_formula_block() {
    assert_eq!(parse("$$x^2$$\n").blocks, [formula("x^2\n", 1)]);
    let document = parse("Intro\n\n$$\n\\frac{a}{b}\n$$\n");
    assert_eq!(
        document.blocks,
        [paragraph("Intro"), formula("\\frac{a}{b}\n", 3)]
    );
    assert_eq!(lines_of(&document, &[&[0], &[1]]), [Some(1), Some(3)]);
}

#[test]
fn a_math_code_block_is_a_formula_block() {
    assert_eq!(parse("```math\nx^2\n```\n").blocks, [formula("x^2\n", 1)]);
}

#[test]
fn display_math_splits_the_paragraph_it_is_in() {
    let document = parse("Before\n$$ a + b $$\nafter *it*.\n");
    assert_eq!(
        document.blocks,
        [
            paragraph("Before"),
            formula("a + b\n", 2),
            Block::Paragraph(vec![
                text("after "),
                Inline::Emphasis(vec![text("it")]),
                text("."),
            ]),
        ]
    );
    assert_eq!(
        lines_of(&document, &[&[0], &[1], &[2]]),
        [Some(1), Some(2), Some(3)]
    );
}

#[test]
fn display_math_in_a_list_item_or_a_quote_is_a_formula_block() {
    let document = parse("- Item\n\n  $$\n  x\n  $$\n\n> $$y$$\n");
    let [Block::List(list), Block::BlockQuote(quote)] = document.blocks.as_slice() else {
        panic!("{:?}", document.blocks);
    };
    assert_eq!(list.items[0].blocks, [paragraph("Item"), formula("x\n", 3)]);
    assert_eq!(quote, &[formula("y\n", 7)]);
    let mut lines = Vec::new();
    code_lines(&document.blocks, &mut lines);
    assert_eq!(lines, [3, 7]);
}

#[test]
fn unterminated_display_math_stays_text() {
    assert_eq!(parse("$$x^2\n").blocks, [paragraph("$$x^2")]);
}

#[test]
fn empty_display_math_stays_text() {
    assert_eq!(parse("$$ $$\n").blocks, [paragraph("$$ $$")]);
    assert_eq!(
        parse("$$\n$$\n").blocks,
        [Block::Paragraph(vec![
            text("$$"),
            Inline::SoftBreak,
            text("$$")
        ])]
    );
    assert_eq!(parse("a $$ $$ b\n").blocks, [paragraph("a $$ $$ b")]);
}

/// The blocks of `source` parsed as before kiri read math.
fn without_math(source: &str) -> Vec<Block> {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &options_without_math());
    blocks(root).into_iter().map(|(block, _)| block).collect()
}

#[test]
fn display_math_in_headings_cells_and_links_stays_text() {
    // Only a paragraph is split at a formula.
    for source in [
        "# A $$x$$ b\n",
        "| a |\n|---|\n| $$x$$ |\n",
        "*a $$x$$ b*\n",
        "[a $$x$$ b](u)\n",
    ] {
        assert_eq!(parse(source).blocks, without_math(source), "{source}");
    }
}

fn inline_formula(source: &str, written: Vec<Inline>) -> Inline {
    Inline::Formula {
        source: source.to_owned(),
        written,
    }
}

#[test]
fn reads_inline_dollar_math_as_a_formula() {
    assert_eq!(
        parse("Let $x^2$ be one.\n").blocks,
        [Block::Paragraph(vec![
            text("Let "),
            inline_formula("x^2", vec![text("$x^2$")]),
            text(" be one."),
        ])]
    );
    // The backtick form GitHub has for formulas with Markdown in them.
    assert_eq!(
        parse("A $`a_1`$ b.\n").blocks,
        [Block::Paragraph(vec![
            text("A "),
            inline_formula(
                "a_1",
                vec![text("$"), Inline::Code("a_1".into()), text("$")]
            ),
            text(" b."),
        ])]
    );
}

#[test]
fn a_formula_keeps_what_it_reads_as_without_math() {
    assert_eq!(
        parse("A $*b*$ c.\n").blocks,
        [Block::Paragraph(vec![
            text("A "),
            inline_formula(
                "*b*",
                vec![text("$"), Inline::Emphasis(vec![text("b")]), text("$")]
            ),
            text(" c."),
        ])]
    );
}

#[test]
fn dollars_that_are_not_math_by_githubs_rules_stay_text() {
    // GitHub opens a formula at a `$` followed by no space and closes it
    // at one preceded by no space and followed by no digit.
    for source in [
        "Cost $5 and $10 today.",
        "A $ x$ b.",
        "A $x $ b.",
        "A $x$5 b.",
        "A $5 b.",
        "Costs 5$ and 6$.",
        "A \\$x$ b.",
        "A `$x$` b.",
    ] {
        let blocks = parse(source).blocks;
        assert!(
            matches!(blocks.as_slice(), [Block::Paragraph(content)]
                if !content.iter().any(|i| matches!(i, Inline::Formula { .. }))),
            "{source}: {blocks:?}"
        );
    }
    // By the same rules two variables can be a formula, as on GitHub.
    assert_eq!(
        parse("$HOME/$USER\n").blocks,
        [Block::Paragraph(vec![
            inline_formula("HOME/", vec![text("$HOME/$")]),
            text("USER"),
        ])]
    );
    assert_eq!(
        parse("Cost $5 and $10 today.\n").blocks,
        [paragraph("Cost $5 and $10 today.")]
    );
}

#[test]
fn inline_math_in_a_quote_or_a_link_is_a_formula() {
    assert_eq!(
        parse("[a $x$](u)\n").blocks,
        [Block::Paragraph(vec![Inline::Link {
            url: "u".into(),
            content: vec![text("a "), inline_formula("x", vec![text("$x$")])],
        }])]
    );
}
