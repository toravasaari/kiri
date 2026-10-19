//! Markdown source to document model, through comrak's syntax tree.

pub mod csv;
mod html;

use comrak::nodes::{AlertType, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Node, Options, parse_document};

use crate::model::{AlertKind, Alignment, Block, Document, Image, Inline, List, ListItem, Table};

pub fn parse(source: &str) -> Document {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &options());
    Document {
        blocks: blocks(root),
    }
}

fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.alerts = true;
    options
}

fn blocks(parent: Node<'_>) -> Vec<Block> {
    parent.children().filter_map(block).collect()
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
    }
    inlines
}

fn inline(node: Node<'_>) -> Option<Inline> {
    Some(match &node.data().value {
        NodeValue::Text(text) => Inline::Text(text.to_string()),
        NodeValue::Code(code) => Inline::Code(code.literal.clone()),
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
        NodeValue::HtmlInline(tag) => match html::inline_img(tag) {
            Some(img) => image(img),
            None => Inline::Html(tag.clone()),
        },
        _ => return None,
    })
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
    fn keeps_only_the_language_from_the_info_string() {
        let document = parse("```rust ignore\nfn main() {}\n```\n");
        assert_eq!(
            document.blocks,
            vec![Block::CodeBlock {
                language: Some("rust".into()),
                code: "fn main() {}\n".into(),
            }]
        );
    }
}
