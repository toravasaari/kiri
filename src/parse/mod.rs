//! Markdown source to document model, through comrak's syntax tree.

pub mod csv;
mod front_matter;
mod html;

use comrak::nodes::{AlertType, ListType, NodeMath, NodeValue, TableAlignment};
use comrak::{Arena, Node, Options, parse_document};

use self::html::DetailsPart;
use crate::model::{
    AlertKind, Alignment, Block, Document, Image, Inline, List, ListItem, SourceLines, Table,
};

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
    let (mut blocks, positions): (Vec<_>, Vec<_>) = blocks(root).into_iter().unzip();
    // comrak counts lines from the end of the front matter.
    let front_matter_lines = source[..source.len() - markdown.len()]
        .matches('\n')
        .count();
    move_lines(&mut blocks, front_matter_lines);
    // The front matter, when there is any, is the first block.
    let first = usize::from(front_matter.is_some());
    let mut lines = SourceLines::default();
    if front_matter.is_some() {
        lines.insert(vec![0], 1);
    }
    for (index, position) in positions.iter().enumerate() {
        position.record(&mut vec![first + index], front_matter_lines, &mut lines);
    }
    Document {
        blocks: front_matter.into_iter().chain(blocks).collect(),
        lines,
    }
}

/// Where a block starts in the source, and where the blocks inside it do.
/// `children` follow the structure of the block: the blocks of a quote,
/// alert or `<details>`, or the items of a list, each with the blocks of
/// the item as its children. An item is not a block, so it has no line.
struct Position {
    line: Option<usize>,
    children: Vec<Position>,
}

impl Position {
    fn leaf(line: usize) -> Self {
        Self {
            line: Some(line),
            children: Vec::new(),
        }
    }

    /// Records the lines of this block, found at `path`, and of the blocks
    /// inside it in `lines`, `by` lines further down than comrak counted.
    fn record(&self, path: &mut Vec<usize>, by: usize, lines: &mut SourceLines) {
        if let Some(line) = self.line {
            lines.insert(path.clone(), line + by);
        }
        for (index, child) in self.children.iter().enumerate() {
            path.push(index);
            child.record(path, by, lines);
            path.pop();
        }
    }
}

/// `source` as it is, without reading it as Markdown: a document of one
/// code block in `language`, which starts on the first line.
pub fn code(source: &str, language: Option<&str>) -> Document {
    let mut lines = SourceLines::default();
    lines.insert(vec![0], 1);
    Document {
        blocks: vec![Block::CodeBlock {
            language: language.map(str::to_owned),
            code: source.to_owned(),
            line: 1,
        }],
        lines,
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
    let mut options = options_without_math();
    options.extension.math_dollars = true;
    options
}

/// The options without `$` math, with which an inline formula reads as
/// it did before kiri drew formulas.
fn options_without_math() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.alerts = true;
    options.extension.shortcodes = true;
    options
}

fn blocks(parent: Node<'_>) -> Vec<(Block, Position)> {
    join_details(
        parent
            .children()
            .flat_map(|node| match split_at_formulas(node) {
                Some(blocks) => blocks,
                None => block(node).into_iter().collect(),
            }),
    )
}

/// The language of a code block that holds a formula, as ` ```math `
/// names it.
const FORMULA: &str = "math";

/// A paragraph with display math, `$$…$$`, in it as the paragraphs
/// before, between and after its formulas and each formula as a code
/// block in `math`, as GitHub shows them; `None` for any other block.
/// Display math with nothing in it, such as `$$ $$`, stays text.
fn split_at_formulas(node: Node<'_>) -> Option<Vec<(Block, Position)>> {
    let formula = |node: &Node<'_>| match &node.data().value {
        NodeValue::Math(math) if math.display_math => {
            let source = math.literal.trim();
            (!source.is_empty()).then(|| source.to_owned())
        }
        _ => None,
    };
    if !matches!(node.data().value, NodeValue::Paragraph)
        || !node.children().any(|n| formula(&n).is_some())
    {
        return None;
    }
    let mut blocks = Vec::new();
    let mut run = Vec::new();
    for child in node.children() {
        if let Some(source) = formula(&child) {
            blocks.extend(paragraph_of(&run));
            run.clear();
            let line = child.data().sourcepos.start.line;
            let code = Block::CodeBlock {
                language: Some(FORMULA.to_owned()),
                code: format!("{source}\n"),
                line,
            };
            blocks.push((code, Position::leaf(line)));
        } else {
            run.push(child);
        }
    }
    blocks.extend(paragraph_of(&run));
    Some(blocks)
}

/// The paragraph of the inline `nodes` between formulas, without the
/// breaks and spaces at its ends; `None` when nothing else is left.
fn paragraph_of(nodes: &[Node<'_>]) -> Option<(Block, Position)> {
    let blank = |node: &&Node<'_>| match &node.data().value {
        NodeValue::SoftBreak | NodeValue::LineBreak => true,
        NodeValue::Text(text) => text.trim().is_empty(),
        _ => false,
    };
    let start = nodes.iter().position(|node| !blank(&node))?;
    let end = nodes.iter().rposition(|node| !blank(&node))? + 1;
    let mut content = inlines_of(nodes[start..end].iter().copied());
    if let Some(Inline::Text(text)) = content.first_mut() {
        *text = text.trim_start().to_owned();
    }
    if let Some(Inline::Text(text)) = content.last_mut() {
        *text = text.trim_end().to_owned();
    }
    let line = nodes[start].data().sourcepos.start.line;
    Some((Block::Paragraph(content), Position::leaf(line)))
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
///
/// A `<details>` starts where its first tag does, and the blocks read from
/// the HTML inside it start where the HTML block they are in does.
fn join_details(blocks: impl Iterator<Item = (Block, Position)>) -> Vec<(Block, Position)> {
    let mut joined = Vec::new();
    // Each `<details>` still open: its summary, its blocks so far and the
    // line it starts on.
    type Open = (Option<Vec<Inline>>, Vec<(Block, Position)>, usize);
    let mut open: Vec<Open> = Vec::new();
    fn push(joined: &mut Vec<(Block, Position)>, open: &mut [Open], block: (Block, Position)) {
        match open.last_mut() {
            Some((_, blocks, _)) => blocks.push(block),
            None => joined.push(block),
        }
    }
    for (block, position) in blocks {
        let line = position.line.unwrap_or(1);
        if let Block::Html(source) = &block {
            if let Some(parts) = html::details_parts(source)
                && reads_details(&parts, !open.is_empty())
            {
                for part in parts {
                    match part {
                        DetailsPart::Start(summary) => {
                            open.push((summary.map(html_inlines), Vec::new(), line));
                        }
                        DetailsPart::End(tag) => match open.pop() {
                            Some((summary, blocks, line)) => {
                                push(&mut joined, &mut open, details(summary, blocks, line));
                            }
                            // A `</details>` with none open stays source.
                            None => push(
                                &mut joined,
                                &mut open,
                                (Block::Html(tag.to_owned()), Position::leaf(line)),
                            ),
                        },
                        DetailsPart::Html(body) => {
                            for block in details_body(body) {
                                push(&mut joined, &mut open, (block, Position::leaf(line)));
                            }
                        }
                    }
                }
                continue;
            }
            // A `<summary>` after a blank line is still the summary when
            // nothing came before it.
            if let Some((summary @ None, blocks, _)) = open.last_mut()
                && blocks.is_empty()
                && let Some(content) = html::summary(source)
            {
                *summary = Some(html_inlines(content));
                continue;
            }
        }
        push(&mut joined, &mut open, (block, position));
    }
    while let Some((summary, blocks, line)) = open.pop() {
        push(&mut joined, &mut open, details(summary, blocks, line));
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
/// without, that starts on `line`.
fn details(
    summary: Option<Vec<Inline>>,
    blocks: Vec<(Block, Position)>,
    line: usize,
) -> (Block, Position) {
    let (blocks, children): (Vec<_>, Vec<_>) = blocks.into_iter().unzip();
    let block = Block::Details {
        summary: summary.unwrap_or_else(|| vec![Inline::Text("Details".into())]),
        blocks,
    };
    let position = Position {
        line: Some(line),
        children,
    };
    (block, position)
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
fn block(node: Node<'_>) -> Option<(Block, Position)> {
    let mut children = Vec::new();
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
        NodeValue::BlockQuote => {
            let (blocks, positions): (Vec<_>, Vec<_>) = blocks(node).into_iter().unzip();
            children = positions;
            Block::BlockQuote(blocks)
        }
        NodeValue::Alert(alert) => {
            let (blocks, positions): (Vec<_>, Vec<_>) = blocks(node).into_iter().unzip();
            children = positions;
            Block::Alert {
                kind: match alert.alert_type {
                    AlertType::Note => AlertKind::Note,
                    AlertType::Tip => AlertKind::Tip,
                    AlertType::Important => AlertKind::Important,
                    AlertType::Warning => AlertKind::Warning,
                    AlertType::Caution => AlertKind::Caution,
                },
                title: alert.title.clone(),
                blocks,
            }
        }
        NodeValue::List(list) => {
            let (items, positions) = node.children().map(list_item).unzip();
            children = positions;
            Block::List(List {
                start: (list.list_type == ListType::Ordered).then_some(list.start as u64),
                tight: list.tight,
                items,
            })
        }
        NodeValue::Table(table) => Block::Table(self::table(node, &table.alignments)),
        NodeValue::ThematicBreak => Block::ThematicBreak,
        NodeValue::HtmlBlock(block) => match html::block_img(&block.literal) {
            Some(img) => Block::Paragraph(vec![linked_image(img)]),
            None => Block::Html(block.literal.clone()),
        },
        _ => return None,
    };
    let position = Position {
        line: Some(node.data().sourcepos.start.line),
        children,
    };
    Some((block, position))
}

fn list_item(node: Node<'_>) -> (ListItem, Position) {
    let checked = match &node.data().value {
        NodeValue::TaskItem(task) => Some(task.symbol.is_some()),
        _ => None,
    };
    let (blocks, children): (Vec<_>, Vec<_>) = blocks(node).into_iter().unzip();
    let position = Position {
        line: None,
        children,
    };
    (ListItem { checked, blocks }, position)
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
    inlines_of(parent.children())
}

fn inlines_of<'a>(nodes: impl Iterator<Item = Node<'a>>) -> Vec<Inline> {
    let mut inlines = Vec::new();
    // Whether the last node was math read as text, which the text around
    // it joins.
    let mut after_math = false;
    for node in nodes {
        if let NodeValue::Math(math) = &node.data().value {
            if math.display_math {
                for inline in math_as_written(math) {
                    push_joined(&mut inlines, inline);
                }
                after_math = true;
            } else {
                inlines.push(Inline::Formula {
                    source: formula_source(&math.literal),
                    written: math_as_written(math),
                });
                after_math = false;
            }
        } else if let Some(inline) = inline(node) {
            match after_math {
                true => push_joined(&mut inlines, inline),
                false => push_inline(&mut inlines, inline),
            }
            after_math = false;
        }
    }
    inlines
}

/// The LaTeX of an inline formula written as `literal` between dollars:
/// the text itself, or what is in the backticks of GitHub's `$`x`$`, which
/// keeps Markdown from reading the formula.
fn formula_source(literal: &str) -> String {
    let source = literal.trim();
    let inner = source
        .strip_prefix('`')
        .and_then(|rest| rest.strip_suffix('`'))
        .filter(|inner| !inner.is_empty() && !inner.contains('`'));
    inner.unwrap_or(source).trim().to_owned()
}

/// Math that kiri does not draw, such as `$x$`, read as it was before
/// kiri read math: as the Markdown it was written in.
fn math_as_written(math: &NodeMath) -> Vec<Inline> {
    let dollars = if math.display_math { "$$" } else { "$" };
    let source = format!("{dollars}{}{dollars}", math.literal);
    let arena = Arena::new();
    let root = parse_document(&arena, &source, &options_without_math());
    root.first_child().map(inlines).unwrap_or_default()
}

/// Pushes `inline`, joining it to the text before it when both are text.
fn push_joined(inlines: &mut Vec<Inline>, inline: Inline) {
    match (inlines.last_mut(), inline) {
        (Some(Inline::Text(before)), Inline::Text(text)) => before.push_str(&text),
        (_, inline) => push_inline(inlines, inline),
    }
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
            html::Phrase::Sub => Inline::Subscript(content),
            html::Phrase::Sup => Inline::Superscript(content),
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
mod tests;
