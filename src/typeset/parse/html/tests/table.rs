//! The reading of the HTML in a document as blocks, tested by one table:
//! each row is a document and the blocks that `parse` reads from it, with
//! their places.

use std::fmt::Write;

use super::rows;
use crate::typeset::model::{Document, Extent};
use crate::typeset::parse::*;

/// A document of the table, and its blocks as `outline` writes them.
pub struct Row {
    pub name: &'static str,
    pub source: &'static str,
    /// The outline, indented as the code round it is.
    pub blocks: &'static str,
}

/// Every row reads as its blocks, at the level "all".
#[test]
fn every_row_of_the_table_reads_as_its_blocks() {
    check(rows::ROWS, HtmlLevel::All, HtmlTags::Dim);
}

/// Every row of what GitHub's level changes reads as its blocks there.
#[test]
fn every_row_of_githubs_level_reads_as_its_blocks() {
    check(rows::GITHUB_ROWS, HtmlLevel::Github, HtmlTags::Dim);
}

/// Every row of what `html_tags = "hide"` changes reads as its blocks, at
/// the default level and at "all".
#[test]
fn every_row_of_hidden_tags_reads_as_its_blocks() {
    check(rows::HIDE_ROWS, HtmlLevel::Github, HtmlTags::Hide);
}

/// The same rows at the level "all", where `<u>` and `<center>` are not
/// tags with no effect: the rows that say so are in `HIDE_ROWS_ALL`.
#[test]
fn every_row_of_hidden_tags_at_all_reads_as_its_blocks() {
    check(rows::HIDE_ROWS_ALL, HtmlLevel::All, HtmlTags::Hide);
}

/// The default level is GitHub's: `<u>` and `<center>` are dim tags at it,
/// and read as they were at "all".
#[test]
fn the_default_level_is_githubs() {
    assert_eq!(HtmlLevel::default(), HtmlLevel::Github);
    assert_eq!(ParseOptions::default().html, HtmlLevel::Github);
    let source = "<center>Text</center>\n\nA <u>b</u> c\n";
    let blocks = |options: &ParseOptions| parse_with_warnings(source, options).0.blocks;
    assert_eq!(blocks(&ParseOptions::default()), parse(source).blocks);
    let all = ParseOptions {
        html: HtmlLevel::All,
        ..ParseOptions::default()
    };
    assert_ne!(blocks(&all), parse(source).blocks);
    // Dim, at the default level: the tags are the same text, and no more.
    let text = format!("{:?}", parse(source).blocks);
    assert!(
        text.contains("Html(\"<center>\")") && !text.contains("Underline"),
        "{text}"
    );
}

/// Each row of `table` reads as its blocks at `level`, and each name is
/// there once.
fn check(table: &[Row], html: HtmlLevel, html_tags: HtmlTags) {
    let options = ParseOptions {
        html,
        html_tags,
        ..ParseOptions::default()
    };
    let differ: Vec<String> = (table.iter())
        .filter_map(|row| {
            let (expected, found) = (dedent(row.blocks), outline_with(row.source, &options));
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
        table.len(),
        differ.join("\n")
    );
    for (at, row) in table.iter().enumerate() {
        assert!(
            table[..at].iter().all(|other| other.name != row.name),
            "{} twice",
            row.name
        );
    }
}

/// The shapes of heads in real READMEs (`tests/fixtures/html-readme-shapes.md`)
/// as `parse` reads them today. The tree of HTML (roadmap 4.10.2) changes
/// the reading of several of them; this is the baseline it is measured by.
#[test]
fn the_shapes_of_real_readmes_read_as_they_do_today() {
    let source = include_str!("../../../../../tests/fixtures/html-readme-shapes.md");
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
///
/// The rows of the table are read at the level "all"; `outline_with` reads
/// with other options.
fn outline(source: &str) -> String {
    let options = ParseOptions {
        html: HtmlLevel::All,
        ..ParseOptions::default()
    };
    outline_with(source, &options)
}

/// The outline of `source` read with `options`.
fn outline_with(source: &str, options: &ParseOptions) -> String {
    let document = parse_with_warnings(source, options).0;
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
        Block::List(list) => {
            let start = list.start.map(|start| format!(" start={start}"));
            let loose = (!list.tight).then_some(" loose");
            format!(
                "list{}{}",
                start.unwrap_or_default(),
                loose.unwrap_or_default()
            )
        }
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
            Inline::Underline(inner) => format!("[u]{}[/u]", words(inner)),
            Inline::Strikethrough(inner) => format!("~~{}~~", words(inner)),
            other => format!("{other:?}"),
        })
        .collect()
}

// The growth tests below are the ones that the tree of HTML answers (roadmap
// 4.10.2): three of them failed on the reading before it.

#[test]
fn plain_divs_one_in_another_cut_by_blank_lines_are_read_in_linear_time() {
    let plain = |n| format!("{}{}", "<div>\n\n".repeat(n), "</div>\n\n".repeat(n));
    let growth_plain = growth(plain, 500);
    assert!(growth_plain < 24.0, "plain: {growth_plain}");
}

#[test]
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
fn many_items_and_many_lists_are_read_in_linear_time() {
    let items = growth(|n| format!("<ul>\n{}</ul>\n", "<li>a\n".repeat(n)), 1_000);
    assert!(items < 24.0, "items without an end tag: {items}");
    let lists = growth(|n| "<ol><li>a</li></ol>".repeat(n), 500);
    assert!(lists < 24.0, "lists: {lists}");
}

#[test]
fn many_details_in_one_html_block_are_read_in_linear_time() {
    let growth_one_block = growth(|n| "<details>t</details>".repeat(n), 500);
    assert!(growth_one_block < 24.0, "{growth_one_block}");
}

/// A document whose elements nest as deep as the tree lets them, 256, lays
/// out in both layouts: the walks of the layouts are bounded by the cap.
#[test]
fn a_document_nested_to_the_cap_lays_out_in_both_layouts() {
    use std::rc::Rc;

    use crate::typeset::caps::Capabilities;
    use crate::typeset::driver::{self, Content};
    use crate::typeset::inputs::Inputs;
    use crate::typeset::reading::engine::Reading;

    let depth = |blocks: &[Block]| {
        let mut depth = 0;
        let mut blocks = blocks;
        loop {
            blocks = match blocks.first() {
                Some(Block::Centered(inner) | Block::Details { blocks: inner, .. }) => inner,
                // A list is as deep as its item is, and the first item holds
                // the next list.
                Some(Block::List(list)) => &list.items[0].blocks,
                _ => return depth,
            };
            depth += 1;
        }
    };
    let cells = Inputs::new(Capabilities::kitty(60));
    let reading = Inputs {
        reading: Some(Reading {
            pictures: Default::default(),
            glyphs: Default::default(),
            fonts: Rc::new(crate::typeset::reading::Fonts::fixture()),
            wide: crate::typeset::reading::page::Wide::Wrap,
            snap: false,
            zoom: 100,
        }),
        caps: Capabilities {
            cell_pixels: Some((10, 20)),
            ..Capabilities::kitty(60)
        },
        ..Inputs::new(Capabilities::kitty(60))
    };
    // A `<ul>` and its `<li>` are two elements open at once, so the same cap
    // lets a list nest half as deep.
    for (open, end, levels) in [
        ("<div align=\"center\">", "</div>", 256),
        ("<div align=\"center\">\n\n", "</div>\n\n", 256),
        ("<details>", "</details>", 256),
        ("<details>\n\n", "</details>\n\n", 256),
        ("<ul><li>", "</li></ul>", 128),
        ("<ul>\n<li>\n\n", "</li>\n</ul>\n\n", 128),
        ("<ol><li>", "</li></ol>", 128),
    ] {
        let source = format!("{}deep\n\n{}", open.repeat(300), end.repeat(300));
        let document = parse(&source);
        assert_eq!(depth(&document.blocks), levels, "{open:?}");
        for inputs in [&cells, &reading] {
            let laid = driver::laid(Content::Document(&document), inputs);
            assert!(!laid.is_empty(), "{open:?}");
        }
    }
}
