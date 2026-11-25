//! The joiner of wrappers, tested by one table: each row is a document
//! and the blocks that `parse` reads from it, with their places.

use std::fmt::Write;

use crate::typeset::model::{Document, Extent};
use crate::typeset::parse::*;

mod rows;

/// A document of the table, and its blocks as `outline` writes them.
pub struct Row {
    pub name: &'static str,
    pub source: &'static str,
    /// The outline, indented as the code round it is.
    pub blocks: &'static str,
}

#[test]
fn every_row_of_the_table_reads_as_its_blocks() {
    let differ: Vec<String> = (rows::ROWS.iter())
        .filter_map(|row| {
            let (expected, found) = (dedent(row.blocks), outline(row.source));
            (found != expected).then(|| {
                format!(
                    "{}\n{:?}\nexpected:\n{expected}found:\n{found}",
                    row.name, row.source
                )
            })
        })
        .collect();
    assert!(
        differ.is_empty(),
        "{} of {} rows differ:\n\n{}",
        differ.len(),
        rows::ROWS.len(),
        differ.join("\n")
    );
}

/// `text` without the blank lines round it and the indent its lines share,
/// each line ending in a line break.
fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let indent = (lines.iter())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|line| format!("{}\n", &line[indent..]))
        .collect()
}

/// The blocks of `source` as `parse` reads it, one line each, indented
/// under the block they are in, each with its place: `@` the line, `=`
/// for exact bytes or `~` for bytes of a larger piece, and the bytes.
fn outline(source: &str) -> String {
    let document = parse(source);
    let mut out = String::new();
    write_blocks(&document, &document.blocks, &mut Vec::new(), 0, &mut out);
    out
}

fn write_blocks(
    document: &Document,
    blocks: &[Block],
    path: &mut Vec<usize>,
    depth: usize,
    out: &mut String,
) {
    for (index, block) in blocks.iter().enumerate() {
        path.push(index);
        let place = match document.places.get(path) {
            Some(place) => {
                let extent = if place.extent == Extent::Exact {
                    '='
                } else {
                    '~'
                };
                format!("@{}{extent}{:?}", place.line, place.bytes)
            }
            None => "@-".to_owned(),
        };
        let indent = "  ".repeat(depth);
        writeln!(out, "{indent}{} {place}", what(block)).unwrap();
        match block {
            Block::Centered(inner)
            | Block::BlockQuote(inner)
            | Block::Details { blocks: inner, .. } => {
                write_blocks(document, inner, path, depth + 1, out);
            }
            Block::List(list) => {
                for (item, ListItem { blocks, .. }) in list.items.iter().enumerate() {
                    writeln!(out, "{indent}  item").unwrap();
                    path.push(item);
                    write_blocks(document, blocks, path, depth + 2, out);
                    path.pop();
                }
            }
            _ => {}
        }
        path.pop();
    }
}

/// A block in a few words.
fn what(block: &Block) -> String {
    match block {
        Block::Paragraph(content) => format!("p {:?}", words(content)),
        Block::Heading { level, content, id } => {
            let id = id.as_ref().map(|id| format!(" #{id}")).unwrap_or_default();
            format!("h{level} {:?}{id}", words(content))
        }
        Block::Html(source) => format!("html {source:?}"),
        Block::Centered(_) => "centred".to_owned(),
        Block::Details { summary, .. } => format!("details {:?}", words(summary)),
        Block::BlockQuote(_) => "quote".to_owned(),
        Block::List(_) => "list".to_owned(),
        Block::CodeBlock { code, .. } => format!("code {code:?}"),
        other => format!("{other:?}"),
    }
}

/// Inlines as Markdown-like text.
fn words(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Text(text) => text.clone(),
            Inline::Emphasis(inner) => format!("*{}*", words(inner)),
            Inline::Strong(inner) => format!("**{}**", words(inner)),
            Inline::Kbd(inner) => format!("<kbd>{}</kbd>", words(inner)),
            Inline::Link { url, content, .. } => format!("[{}]({url})", words(content)),
            Inline::Image(image) => format!("![{}]({})", words(&image.alt), image.url),
            Inline::LineBreak => "<br>".to_owned(),
            Inline::SoftBreak => " ".to_owned(),
            Inline::Html(html) => html.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}
