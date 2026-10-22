//! Markdown source to document model, through comrak's syntax tree.

pub mod csv;
mod front_matter;
mod html;

use comrak::nodes::{AlertType, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Node, Options, parse_document};

use self::html::DetailsPart;
use crate::model::{AlertKind, Alignment, Block, Document, Image, Inline, List, ListItem, Table};

/// Parses Markdown that may start with front matter.
pub fn parse(source: &str) -> Document {
    parse_with(source, true)
}

/// Parses Markdown, with or without recognising `front_matter` at its
/// start.
pub fn parse_with(source: &str, front_matter: bool) -> Document {
    let split = front_matter.then(|| front_matter::split(source)).flatten();
    let (front_matter, markdown) = match split {
        Some((front_matter, markdown)) => (Some(Block::FrontMatter(front_matter)), markdown),
        None => (None, source),
    };
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &options());
    let mut blocks = blocks(root);
    // comrak counts lines from the end of the front matter.
    let front_matter_lines = source[..source.len() - markdown.len()]
        .matches('\n')
        .count();
    move_lines(&mut blocks, front_matter_lines);
    Document {
        blocks: front_matter.into_iter().chain(blocks).collect(),
    }
}

/// `source` as it is, without reading it as Markdown: a document of one
/// code block in `language`, which starts on the first line.
pub fn code(source: &str, language: Option<&str>) -> Document {
    Document {
        blocks: vec![Block::CodeBlock {
            language: language.map(str::to_owned),
            code: source.to_owned(),
            line: 1,
        }],
    }
}

/// Moves the line of each code block in `blocks` `by` lines down.
fn move_lines(blocks: &mut [Block], by: usize) {
    for block in blocks {
        match block {
            Block::CodeBlock { line, .. } => *line += by,
            Block::BlockQuote(blocks)
            | Block::Alert { blocks, .. }
            | Block::Details { blocks, .. } => move_lines(blocks, by),
            Block::List(list) => {
                for item in &mut list.items {
                    move_lines(&mut item.blocks, by);
                }
            }
            Block::FrontMatter(_)
            | Block::Heading { .. }
            | Block::Paragraph(_)
            | Block::Table(_)
            | Block::ThematicBreak
            | Block::Html(_)
            | Block::Note(_)
            | Block::Error(_)
            | Block::ImageFile(_) => {}
        }
    }
}

fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.alerts = true;
    options.extension.shortcodes = true;
    options
}

fn blocks(parent: Node<'_>) -> Vec<Block> {
    join_details(parent.children().filter_map(block))
}

/// `blocks` with each `<details>` in them as one block. comrak gives a
/// `<details>` with blank lines in it as an HTML block that starts it,
/// the blocks inside and an HTML block that ends it. One that never ends
/// runs to the end of `blocks`, as in a browser.
///
/// comrak ends an HTML block only at a blank line, so a block can hold
/// more than a `<details>` with its `<summary>`, or more than a
/// `</details>`. A block that starts with a `<details>`, or that holds a
/// `</details>` while one is open, is read tag by tag; the HTML between
/// its tags is read as the body of a `<details>` in one block is.
fn join_details(blocks: impl Iterator<Item = Block>) -> Vec<Block> {
    let mut joined = Vec::new();
    // Each `<details>` still open: its summary and its blocks so far.
    let mut open: Vec<(Option<Vec<Inline>>, Vec<Block>)> = Vec::new();
    fn push(joined: &mut Vec<Block>, open: &mut [(Option<Vec<Inline>>, Vec<Block>)], block: Block) {
        match open.last_mut() {
            Some((_, blocks)) => blocks.push(block),
            None => joined.push(block),
        }
    }
    for block in blocks {
        if let Block::Html(source) = &block {
            if let Some(parts) = html::details_parts(source)
                && reads_details(&parts, !open.is_empty())
            {
                for part in parts {
                    match part {
                        DetailsPart::Start(summary) => {
                            open.push((summary.map(html_inlines), Vec::new()));
                        }
                        DetailsPart::End(tag) => match open.pop() {
                            Some((summary, blocks)) => {
                                push(&mut joined, &mut open, details(summary, blocks));
                            }
                            // A `</details>` with none open stays source.
                            None => push(&mut joined, &mut open, Block::Html(tag.to_owned())),
                        },
                        DetailsPart::Html(body) => {
                            for block in details_body(body) {
                                push(&mut joined, &mut open, block);
                            }
                        }
                    }
                }
                continue;
            }
            // A `<summary>` after a blank line is still the summary when
            // nothing came before it.
            if let Some((summary @ None, blocks)) = open.last_mut()
                && blocks.is_empty()
                && let Some(content) = html::summary(source)
            {
                *summary = Some(html_inlines(content));
                continue;
            }
        }
        push(&mut joined, &mut open, block);
    }
    while let Some((summary, blocks)) = open.pop() {
        push(&mut joined, &mut open, details(summary, blocks));
    }
    joined
}

/// Whether `join_details` reads an HTML block of `parts` tag by tag: when
/// it starts with a `<details>`, or holds a `</details>` and one is `open`.
fn reads_details(parts: &[DetailsPart], open: bool) -> bool {
    match parts {
        [DetailsPart::Start(_), ..] => true,
        _ => open && (parts.iter()).any(|part| matches!(part, DetailsPart::End(_))),
    }
}

/// A `<details>` with `summary`, or with the one a browser gives it
/// without.
fn details(summary: Option<Vec<Inline>>, blocks: Vec<Block>) -> Block {
    Block::Details {
        summary: summary.unwrap_or_else(|| vec![Inline::Text("Details".into())]),
        blocks,
    }
}

/// The blocks of HTML inside a `<details>` that is not Markdown: an image,
/// or its text as one paragraph.
fn details_body(body: &str) -> Vec<Block> {
    if let Some(img) = html::block_img(body) {
        return vec![Block::Paragraph(vec![linked_image(img)])];
    }
    match html::plain_text(body) {
        Some(text) if !text.is_empty() => vec![Block::Paragraph(vec![Inline::Text(text)])],
        _ => Vec::new(),
    }
}

// Block types without a model counterpart yet are skipped.
fn block(node: Node<'_>) -> Option<Block> {
    let block = match &node.data().value {
        NodeValue::Heading(heading) => Block::Heading {
            level: heading.level,
            content: inlines(node),
        },
        NodeValue::Paragraph => Block::Paragraph(inlines(node)),
        NodeValue::CodeBlock(code) => Block::CodeBlock {
            language: code.info.split_whitespace().next().map(str::to_owned),
            code: code.literal.clone(),
            line: node.data().sourcepos.start.line,
        },
        NodeValue::BlockQuote => Block::BlockQuote(blocks(node)),
        NodeValue::Alert(alert) => Block::Alert {
            kind: match alert.alert_type {
                AlertType::Note => AlertKind::Note,
                AlertType::Tip => AlertKind::Tip,
                AlertType::Important => AlertKind::Important,
                AlertType::Warning => AlertKind::Warning,
                AlertType::Caution => AlertKind::Caution,
            },
            title: alert.title.clone(),
            blocks: blocks(node),
        },
        NodeValue::List(list) => Block::List(List {
            start: (list.list_type == ListType::Ordered).then_some(list.start as u64),
            tight: list.tight,
            items: node.children().map(list_item).collect(),
        }),
        NodeValue::Table(table) => Block::Table(self::table(node, &table.alignments)),
        NodeValue::ThematicBreak => Block::ThematicBreak,
        NodeValue::HtmlBlock(block) => match html::block_img(&block.literal) {
            Some(img) => Block::Paragraph(vec![linked_image(img)]),
            None => Block::Html(block.literal.clone()),
        },
        _ => return None,
    };
    Some(block)
}

fn list_item(node: Node<'_>) -> ListItem {
    let checked = match &node.data().value {
        NodeValue::TaskItem(task) => Some(task.symbol.is_some()),
        _ => None,
    };
    ListItem {
        checked,
        blocks: blocks(node),
    }
}

fn table(node: Node<'_>, alignments: &[TableAlignment]) -> Table {
    let mut table = Table {
        alignments: alignments.iter().map(|a| alignment(*a)).collect(),
        header: Vec::new(),
        rows: Vec::new(),
    };
    for row in node.children() {
        let is_header = matches!(row.data().value, NodeValue::TableRow(true));
        let cells: Vec<Vec<Inline>> = row.children().map(inlines).collect();
        if is_header {
            table.header = cells;
        } else {
            table.rows.push(cells);
        }
    }
    table
}

fn alignment(alignment: TableAlignment) -> Alignment {
    match alignment {
        TableAlignment::Center => Alignment::Center,
        TableAlignment::Right => Alignment::Right,
        TableAlignment::None | TableAlignment::Left => Alignment::Left,
    }
}

fn inlines(parent: Node<'_>) -> Vec<Inline> {
    let mut inlines = Vec::new();
    for inline in parent.children().filter_map(inline) {
        push_inline(&mut inlines, inline);
    }
    inlines
}

/// Text and inline HTML, as `html` gives the content of a `<summary>`,
/// with the tags kiri reads read.
fn html_inlines(content: Vec<Inline>) -> Vec<Inline> {
    let mut inlines = Vec::new();
    for inline in content {
        let inline = match inline {
            Inline::Html(tag) => html_inline(tag),
            inline => inline,
        };
        push_inline(&mut inlines, inline);
    }
    inlines
}

/// Pushes `inline`, joining it with the inlines before it into the
/// element that HTML tags among them make.
fn push_inline(inlines: &mut Vec<Inline>, inline: Inline) {
    inlines.push(inline);
    // `<a href>`, an `<img>` and `</a>` are a link around the image, as
    // `[![]()]()` is.
    if let [.., Inline::Html(start), Inline::Image(_), Inline::Html(end)] = inlines.as_slice()
        && html::is_link_end(end)
        && let Some(url) = html::link_start(start)
    {
        inlines.pop();
        let image = inlines.pop().expect("the image");
        inlines.pop();
        inlines.push(Inline::Link {
            url,
            content: vec![image],
        });
    }
    // A closing tag such as `</kbd>` ends the element its last opening
    // tag among these inlines started.
    if let Some(Inline::Html(end)) = inlines.last()
        && let Some(phrase) = html::phrase_end(end)
        && let Some(start) = inlines.iter().rposition(|inline| {
            matches!(inline, Inline::Html(start) if html::phrase_start(start) == Some(phrase))
        })
    {
        inlines.pop();
        let content: Vec<Inline> = inlines.drain(start..).skip(1).collect();
        inlines.push(match phrase {
            html::Phrase::Kbd => Inline::Kbd(content),
            html::Phrase::Mark => Inline::Mark(content),
        });
    }
}

fn inline(node: Node<'_>) -> Option<Inline> {
    Some(match &node.data().value {
        NodeValue::Text(text) => Inline::Text(text.to_string()),
        NodeValue::Code(code) => Inline::Code(code.literal.clone()),
        NodeValue::ShortCode(shortcode) => Inline::Text(shortcode.emoji.clone()),
        NodeValue::Emph => Inline::Emphasis(inlines(node)),
        NodeValue::Strong => Inline::Strong(inlines(node)),
        NodeValue::Strikethrough => Inline::Strikethrough(inlines(node)),
        NodeValue::Link(link) => Inline::Link {
            url: link.url.clone(),
            content: inlines(node),
        },
        NodeValue::Image(link) => Inline::Image(Image {
            url: link.url.clone(),
            alt: inlines(node),
            width: None,
            height: None,
        }),
        NodeValue::SoftBreak => Inline::SoftBreak,
        NodeValue::LineBreak => Inline::LineBreak,
        NodeValue::HtmlInline(tag) => html_inline(tag.clone()),
        _ => return None,
    })
}

/// One tag of inline HTML: a line break, an image, or else the tag.
fn html_inline(tag: String) -> Inline {
    if html::is_line_break(&tag) {
        return Inline::LineBreak;
    }
    match html::inline_img(&tag) {
        Some(img) => image(img),
        None => Inline::Html(tag),
    }
}

/// An HTML `<img>` as Markdown would give it: the image, or a link around
/// it when the HTML had one.
fn linked_image(mut img: html::Img) -> Inline {
    match img.link.take() {
        Some(url) => Inline::Link {
            url,
            content: vec![image(img)],
        },
        None => image(img),
    }
}

/// An HTML `<img>` as the image Markdown would give.
fn image(img: html::Img) -> Inline {
    let alt = if img.alt.is_empty() {
        Vec::new()
    } else {
        vec![Inline::Text(img.alt)]
    };
    Inline::Image(Image {
        url: img.src,
        alt,
        width: img.width,
        height: img.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Length;

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
                Block::FrontMatter(crate::model::FrontMatter {
                    language: "yaml".into(),
                    source: "title: A\n".into(),
                    value: Some(Ok(crate::model::Value::Map(vec![(
                        "title".into(),
                        crate::model::Value::String("A".into())
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
        let image = || {
            Inline::Image(Image {
                url: "a.png".into(),
                alt: vec![text("An image")],
                width: None,
                height: None,
            })
        };
        assert_eq!(
            parse("<img src=\"a.png\" alt=\"An image\">\n").blocks,
            vec![Block::Paragraph(vec![image()])]
        );
        assert_eq!(
            parse("<p align=\"center\">\n  <img src=\"a.png\" alt=\"An image\">\n</p>\n").blocks,
            vec![Block::Paragraph(vec![image()])]
        );
        assert_eq!(
            parse("Text <img src=\"a.png\" alt=\"An image\"> here.\n").blocks,
            vec![Block::Paragraph(vec![
                text("Text "),
                image(),
                text(" here.")
            ])]
        );
    }

    #[test]
    fn an_img_without_alt_has_no_alt_text() {
        assert_eq!(
            parse("<img src=a.png>\n").blocks,
            vec![Block::Paragraph(vec![Inline::Image(Image {
                url: "a.png".into(),
                alt: Vec::new(),
                width: None,
                height: None,
            })])]
        );
    }

    #[test]
    fn reads_an_html_link_around_an_img_as_a_link() {
        let linked = || Inline::Link {
            url: "https://x".into(),
            content: vec![Inline::Image(Image {
                url: "a.png".into(),
                alt: vec![text("x")],
                width: None,
                height: None,
            })],
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
                Inline::Image(Image {
                    url: "a.png".into(),
                    alt: Vec::new(),
                    width: None,
                    height: None,
                }),
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
            parse("Text <b>bold</b>\n").blocks,
            vec![Block::Paragraph(vec![
                text("Text "),
                Inline::Html("<b>".into()),
                text("bold"),
                Inline::Html("</b>".into()),
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
        let source =
            "<details>\n<summary>S</summary>\n\nIn.\n\n<p>More.</p>\n</details>\n\nAfter.\n";
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
                vec![Block::Paragraph(vec![Inline::Image(Image {
                    url: "a.png".into(),
                    alt: vec![text("x")],
                    width: None,
                    height: None,
                })])]
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
                Inline::Image(Image {
                    url: "a.png".into(),
                    alt: vec![text("\u{1F680}")],
                    width: None,
                    height: None,
                }),
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
}
