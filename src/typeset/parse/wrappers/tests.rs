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

/// The shapes of heads in real READMEs (`tests/fixtures/html-readme-shapes.md`)
/// as `parse` reads them today. The tree of HTML (roadmap 4.10.2) changes
/// the reading of several of them; this is the baseline it is measured by.
#[test]
fn the_shapes_of_real_readmes_read_as_they_do_today() {
    let source = include_str!("../../../../tests/fixtures/html-readme-shapes.md");
    insta::assert_snapshot!(outline(source));
}

/// How the time to parse `make(n)` grows from `n` to eight times `n`
/// (`crate::testing::growth`).
fn growth(make: impl Fn(usize) -> String, n: usize) -> f64 {
    crate::testing::growth(
        |source| {
            parse(source);
        },
        make,
        n,
    )
}

#[test]
fn a_block_of_many_stray_end_tags_is_read_in_linear_time() {
    let alone = growth(|n| "</div>\n".repeat(n), 2_000);
    assert!(alone < 24.0, "{alone}");
    // A `</details>` after them, with no `<details>` open.
    let with_end = growth(|n| format!("{}</details>\n", "</p>\n".repeat(n)), 2_000);
    assert!(with_end < 24.0, "with a details end: {with_end}");
}

#[test]
fn a_block_of_many_summary_tags_is_read_in_linear_time() {
    let open = growth(|n| "<summary>x\n".repeat(n), 1_000);
    assert!(open < 24.0, "without an end tag: {open}");
    let ended = growth(
        |n| format!("{}</summary>\n", "<summary>x\n".repeat(n)),
        1_000,
    );
    assert!(ended < 24.0, "with one at the end: {ended}");
}

#[test]
fn a_block_of_many_centred_elements_is_read_in_linear_time() {
    let growth = growth(|n| "<p align=\"center\">x</p>\n".repeat(n), 1_000);
    assert!(growth < 24.0, "{growth}");
}

#[test]
fn many_wrappers_that_never_end_are_read_in_linear_time() {
    let unclosed = |n| "<div align=\"center\">\n\nA\n\n".repeat(n);
    let growth_at_end = growth(unclosed, 1_000);
    assert!(growth_at_end < 24.0, "at the end: {growth_at_end}");
    let in_details = |n| format!("<details>\n\n{}</details>\n", unclosed(n));
    let growth_in_details = growth(in_details, 1_000);
    assert!(
        growth_in_details < 24.0,
        "in a details: {growth_in_details}"
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

#[test]
fn a_start_tag_ends_a_p_that_its_block_left_open() {
    use crate::typeset::parse::{html, wrappers::left_open};
    let open = |source: &str| -> Vec<&str> {
        (left_open(&html::events(source).expect("events")).iter())
            .map(|element| element.name)
            .collect()
    };
    assert_eq!(open("<p>a<p>b</p>"), Vec::<&str>::new());
    assert_eq!(open("<p>a<div>b"), ["div"]);
    assert_eq!(open("<div><p>a"), ["div", "p"]);
}

#[test]
fn the_tags_of_an_html_block_are_read_once() {
    use crate::typeset::parse::html::EVENTS;
    for source in [
        "<p align=\"center\">x</p>\n",
        "<div align=\"center\">\n\nA\n\n</div>\n",
        "<div>\n<img src=\"a.png\">\n",
    ] {
        let blocks = source
            .split("\n\n")
            .filter(|block| block.contains('<'))
            .count();
        EVENTS.with(|events| events.set(0));
        parse(source);
        assert_eq!(EVENTS.with(std::cell::Cell::get), blocks, "{source:?}");
    }
}

// The growth tests below are the ones that the tree of HTML answers (roadmap
// 4.10.2). Those with `ignore` fail on the reading of today, and are run by
// hand with `--ignored`; the switch to the tree (step 5) takes the `ignore`
// off.

#[test]
#[ignore = "fails until the tree reads HTML (4.10.2 step 5)"]
fn plain_divs_one_in_another_cut_by_blank_lines_are_read_in_linear_time() {
    let plain = |n| format!("{}{}", "<div>\n\n".repeat(n), "</div>\n\n".repeat(n));
    let growth_plain = growth(plain, 500);
    assert!(growth_plain < 24.0, "plain: {growth_plain}");
}

#[test]
#[ignore = "fails until the tree reads HTML (4.10.2 step 5)"]
fn centring_divs_one_in_another_cut_by_blank_lines_are_read_in_linear_time() {
    let centring = |n| {
        let open = "<div align=\"center\">\n\n";
        format!("{}{}", open.repeat(n), "</div>\n\n".repeat(n))
    };
    let growth_centring = growth(centring, 500);
    assert!(growth_centring < 24.0, "centring: {growth_centring}");
}

/// Reads in linear time today as well, as the tree must.
#[test]
fn elements_never_ended_then_stray_end_tags_are_read_in_linear_time() {
    let stray = |n| format!("{}{}", "<div>\n\n".repeat(n), "</p>\n\n".repeat(n));
    let growth_stray = growth(stray, 500);
    assert!(growth_stray < 24.0, "stray end tags: {growth_stray}");
    let blocked = |n| {
        let (open, end) = ("<center>\n\n<details>\n\n", "</center>\n\n");
        format!(
            "{open}{}{}</details>\n",
            "<div>\n\n".repeat(n),
            end.repeat(n)
        )
    };
    let growth_blocked = growth(blocked, 500);
    assert!(growth_blocked < 24.0, "blocked: {growth_blocked}");
}

#[test]
#[ignore = "fails until the tree reads HTML (4.10.2 step 5)"]
fn many_details_in_one_html_block_are_read_in_linear_time() {
    let growth_one_block = growth(|n| "<details>t</details>".repeat(n), 500);
    assert!(growth_one_block < 24.0, "{growth_one_block}");
}
