//! Markdown source to document model, through comrak's syntax tree.

use comrak::nodes::NodeValue;
use comrak::{Arena, Node, Options, parse_document};

use crate::model::{Block, Document, Inline};

pub fn parse(source: &str) -> Document {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &options());
    Document {
        blocks: root.children().filter_map(block).collect(),
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

// Block types without a model counterpart yet are skipped.
fn block(node: Node<'_>) -> Option<Block> {
    match &node.data().value {
        NodeValue::Heading(heading) => Some(Block::Heading {
            level: heading.level,
            content: inlines(node),
        }),
        NodeValue::Paragraph => Some(Block::Paragraph(inlines(node))),
        _ => None,
    }
}

// Until the model has inline styles, every inline is flattened to its text.
fn inlines(node: Node<'_>) -> Vec<Inline> {
    let mut text = String::new();
    collect_text(node, &mut text);
    vec![Inline::Text(text)]
}

fn collect_text(node: Node<'_>, out: &mut String) {
    for child in node.children() {
        match &child.data().value {
            NodeValue::Text(text) => out.push_str(text),
            NodeValue::Code(code) => out.push_str(&code.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
            _ => collect_text(child, out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headings_and_paragraphs() {
        let document = parse("# Title\n\nSome *emphasised*\ntext.\n");
        assert_eq!(
            document.blocks,
            vec![
                Block::Heading {
                    level: 1,
                    content: vec![Inline::Text("Title".into())],
                },
                Block::Paragraph(vec![Inline::Text("Some emphasised text.".into())]),
            ]
        );
    }
}
