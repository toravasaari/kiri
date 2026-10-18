//! Markdown source to document model, through comrak's syntax tree.

use comrak::nodes::{ListType, NodeValue, TableAlignment};
use comrak::{Arena, Node, Options, parse_document};

use crate::model::{Alignment, Block, Document, Inline, List, ListItem, Table};

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
        NodeValue::List(list) => Block::List(List {
            start: (list.list_type == ListType::Ordered).then_some(list.start as u64),
            tight: list.tight,
            items: node.children().map(list_item).collect(),
        }),
        NodeValue::Table(table) => Block::Table(self::table(node, &table.alignments)),
        NodeValue::ThematicBreak => Block::ThematicBreak,
        NodeValue::HtmlBlock(html) => Block::Html(html.literal.clone()),
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
    parent.children().filter_map(inline).collect()
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
        NodeValue::Image(link) => Inline::Image {
            url: link.url.clone(),
            alt: inlines(node),
        },
        NodeValue::SoftBreak => Inline::SoftBreak,
        NodeValue::LineBreak => Inline::LineBreak,
        NodeValue::HtmlInline(html) => Inline::Html(html.clone()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
