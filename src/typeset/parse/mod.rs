//! Markdown source to document model, through comrak's syntax tree.

pub mod csv;
mod front_matter;
mod html;
#[cfg(test)]
mod inline_places;
mod unknown;

use comrak::nodes::{AlertType, ListType, NodeMath, NodeValue, Sourcepos, TableAlignment};
use comrak::{Arena, Node, Options, parse_document};

use self::html::DetailsPart;
use self::unknown::Unknown;
use crate::typeset::diagnostic::Diagnostic;
use crate::typeset::model::{
    AlertKind, Alignment, Block, Document, Extent, Image, Inline, List, ListItem, Place, SourceMap,
    Table,
};

/// Parses Markdown that may start with front matter.
pub fn parse(source: &str) -> Document {
    parse_with(source, true)
}

/// Parses Markdown, with or without recognising `front_matter` at its
/// start.
pub fn parse_with(source: &str, front_matter: bool) -> Document {
    parse_with_warnings(source, front_matter).0
}

/// Parses Markdown as [`parse_with`] does, and tells what it leaves out:
/// a warning for each kind of Markdown element that the document model
/// has no place for, at the line where the first of its kind starts.
pub fn parse_with_warnings(source: &str, front_matter: bool) -> (Document, Vec<Diagnostic>) {
    parse_with_options(source, front_matter, &options())
}

fn parse_with_options(
    source: &str,
    front_matter: bool,
    options: &Options<'_>,
) -> (Document, Vec<Diagnostic>) {
    let split = front_matter.then(|| front_matter::split(source)).flatten();
    let (front_matter, markdown) = match split {
        Some((front_matter, markdown)) => (Some(Block::FrontMatter(front_matter)), markdown),
        None => (None, source),
    };
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, options);
    let mut unknown = Unknown::default();
    let (blocks, positions): (Vec<_>, Vec<_>) = blocks(root, &mut unknown).into_iter().unzip();
    // comrak counts lines from the end of the front matter.
    let front_matter_bytes = source.len() - markdown.len();
    let front_matter_lines = source[..front_matter_bytes].matches('\n').count();
    let text = Text::new(source, front_matter_bytes);
    // The front matter, when there is any, is the first block.
    let first = usize::from(front_matter.is_some());
    let mut places = SourceMap::default();
    if front_matter.is_some() {
        let end = source[..front_matter_bytes]
            .trim_end_matches(['\n', '\r'])
            .len();
        places.insert(
            vec![0],
            Place {
                line: 1,
                bytes: 0..end,
                extent: Extent::Exact,
            },
        );
    }
    for (index, position) in positions.iter().enumerate() {
        position.record(&mut vec![first + index], &text, &mut places);
    }
    let document = Document {
        blocks: front_matter.into_iter().chain(blocks).collect(),
        places,
    };
    (document, unknown.into_warnings(front_matter_lines))
}

/// Where a block was written in the source, and where the blocks inside
/// it were. `children` follow the structure of the block: the blocks of a
/// quote, alert or `<details>`, or the items of a list, each with the
/// blocks of the item as its children. An item is not a block, so it has
/// no place.
struct Position {
    /// What comrak says of the block, and how well that says it.
    at: Option<Found>,
    children: Vec<Position>,
}

/// Where comrak found a block, or the piece of source it was read from.
#[derive(Clone, Copy)]
struct Found {
    /// The source the block was read from.
    sourcepos: Sourcepos,
    /// The line the block itself starts on, which is below where `sourcepos`
    /// starts when the block is part of a larger piece.
    line: usize,
    extent: Extent,
}

impl Position {
    /// A block that is exactly what comrak found at `at`.
    fn exact(at: Sourcepos) -> Self {
        Self {
            at: Some(Found {
                sourcepos: at,
                line: at.start.line,
                extent: Extent::Exact,
            }),
            children: Vec::new(),
        }
    }

    /// A block read from part of what comrak found at `whole`, which
    /// starts on `line`.
    fn inside(whole: Sourcepos, line: usize) -> Self {
        Self {
            at: Some(Found {
                sourcepos: whole,
                line,
                extent: Extent::Inside,
            }),
            children: Vec::new(),
        }
    }

    /// Where the block starts and ends. Every block has it; an item of a
    /// list does not.
    fn sourcepos(&self) -> Sourcepos {
        self.at
            .map_or_else(|| (1, 1, 1, 1).into(), |found| found.sourcepos)
    }

    /// Records the places of this block, found at `path`, and of the
    /// blocks inside it in `places`.
    fn record(&self, path: &mut Vec<usize>, text: &Text<'_>, places: &mut SourceMap) {
        if let Some(found) = self.at {
            places.insert(path.clone(), text.place(found));
        }
        for (index, child) in self.children.iter().enumerate() {
            path.push(index);
            child.record(path, text, places);
            path.pop();
        }
    }
}

/// The lines of a source, to turn the lines and columns that comrak
/// reports into places in the whole source. comrak reads the source without
/// its front matter, so its lines and bytes start where the front matter
/// ends; its columns count bytes from 1.
struct Text<'a> {
    source: &'a str,
    /// The byte each line of what comrak read starts at.
    starts: Vec<usize>,
    /// The lines the front matter takes.
    skipped: usize,
}

impl<'a> Text<'a> {
    fn new(source: &'a str, skipped_bytes: usize) -> Self {
        let mut starts = vec![skipped_bytes];
        let rest = &source[skipped_bytes..];
        starts.extend(
            rest.match_indices('\n')
                .map(|(at, _)| skipped_bytes + at + 1),
        );
        Self {
            source,
            starts,
            skipped: source[..skipped_bytes].matches('\n').count(),
        }
    }

    /// The end of line `line` of what comrak read, before its line break.
    fn line_end(&self, line: usize) -> usize {
        let next = self.starts.get(line).copied().unwrap_or(self.source.len());
        self.source[..next]
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .len()
    }

    fn place(&self, found: Found) -> Place {
        let at = found.sourcepos;
        let len = self.source.len();
        let line_start = |line: usize| self.starts[line.clamp(1, self.starts.len()) - 1];
        let start = (line_start(at.start.line) + at.start.column.saturating_sub(1)).min(len);
        // A block that ends in the first column of a line ends with the
        // line before.
        let end = match at.end.column {
            0 => self.line_end(at.end.line.saturating_sub(1)),
            column => line_start(at.end.line) + column,
        };
        Place {
            line: found.line + self.skipped,
            bytes: start..end.min(len).max(start),
            extent: found.extent,
        }
    }
}

/// `source` as it is, without reading it as Markdown: a document of one
/// code block in `language`, which starts on the first line.
pub fn code(source: &str, language: Option<&str>) -> Document {
    code_block(source, language, false)
}

/// A whole file shown as text: [`code`], drawn bare, as a file and not as
/// a block among others.
pub fn file(source: &str, language: Option<&str>) -> Document {
    code_block(source, language, true)
}

fn code_block(source: &str, language: Option<&str>, whole_file: bool) -> Document {
    let mut places = SourceMap::default();
    places.insert(vec![0], Place::file(source.len()));
    Document {
        blocks: vec![Block::CodeBlock {
            language: language.map(str::to_owned),
            code: source.to_owned(),
            whole_file,
        }],
        places,
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

fn blocks(parent: Node<'_>, unknown: &mut Unknown) -> Vec<(Block, Position)> {
    join_details(
        parent
            .children()
            .flat_map(|node| match split_at_formulas(node, unknown) {
                Some(blocks) => blocks,
                None => block(node, unknown).into_iter().collect(),
            }),
    )
}

/// The language of a code block that holds a formula, as ` ```math `
/// names it.
pub const FORMULA: &str = "math";

/// A paragraph with display math, `$$…$$`, in it as the paragraphs
/// before, between and after its formulas and each formula as a code
/// block in `math`, as GitHub shows them; `None` for any other block.
/// Display math with nothing in it, such as `$$ $$`, stays text.
fn split_at_formulas(node: Node<'_>, unknown: &mut Unknown) -> Option<Vec<(Block, Position)>> {
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
            blocks.extend(paragraph_of(&run, node.data().sourcepos, unknown));
            run.clear();
            let code = Block::CodeBlock {
                language: Some(FORMULA.to_owned()),
                code: format!("{source}\n"),
                whole_file: false,
            };
            blocks.push((
                code,
                Position::inside(node.data().sourcepos, child.data().sourcepos.start.line),
            ));
        } else {
            run.push(child);
        }
    }
    blocks.extend(paragraph_of(&run, node.data().sourcepos, unknown));
    Some(blocks)
}

/// The paragraph of the inline `nodes` between formulas, without the
/// breaks and spaces at its ends; `None` when nothing else is left.
fn paragraph_of(
    nodes: &[Node<'_>],
    whole: Sourcepos,
    unknown: &mut Unknown,
) -> Option<(Block, Position)> {
    let blank = |node: &&Node<'_>| match &node.data().value {
        NodeValue::SoftBreak | NodeValue::LineBreak => true,
        NodeValue::Text(text) => text.trim().is_empty(),
        _ => false,
    };
    let start = nodes.iter().position(|node| !blank(&node))?;
    let end = nodes.iter().rposition(|node| !blank(&node))? + 1;
    let mut content = inlines_of(nodes[start..end].iter().copied(), unknown);
    if let Some(Inline::Text(text)) = content.first_mut() {
        *text = text.trim_start().to_owned();
    }
    if let Some(Inline::Text(text)) = content.last_mut() {
        *text = text.trim_end().to_owned();
    }
    let line = nodes[start].data().sourcepos.start.line;
    Some((Block::Paragraph(content), Position::inside(whole, line)))
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
    // Each `<details>` still open: its summary, its blocks so far and
    // the HTML block its first tag is in.
    type Open = (Option<Vec<Inline>>, Vec<(Block, Position)>, Sourcepos);
    let mut open: Vec<Open> = Vec::new();
    fn push(joined: &mut Vec<(Block, Position)>, open: &mut [Open], block: (Block, Position)) {
        match open.last_mut() {
            Some((_, blocks, _)) => blocks.push(block),
            None => joined.push(block),
        }
    }
    for (block, position) in blocks {
        let at = position.sourcepos();
        if let Block::Html(source) = &block {
            if let Some(parts) = html::details_parts(source)
                && reads_details(&parts, !open.is_empty())
            {
                for part in parts {
                    match part {
                        DetailsPart::Start(summary) => {
                            open.push((summary.map(html_inlines), Vec::new(), at));
                        }
                        DetailsPart::End(tag) => match open.pop() {
                            Some((summary, blocks, first)) => {
                                let end = Some(at.end);
                                let block = details(summary, blocks, first, end);
                                push(&mut joined, &mut open, block);
                            }
                            // A `</details>` with none open stays source.
                            None => push(
                                &mut joined,
                                &mut open,
                                (
                                    Block::Html(tag.to_owned()),
                                    Position::inside(at, at.start.line),
                                ),
                            ),
                        },
                        DetailsPart::Html(body) => {
                            for block in details_body(body) {
                                push(
                                    &mut joined,
                                    &mut open,
                                    (block, Position::inside(at, at.start.line)),
                                );
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
    while let Some((summary, blocks, first)) = open.pop() {
        push(
            &mut joined,
            &mut open,
            details(summary, blocks, first, None),
        );
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
/// without, that starts where the HTML block `first` does and ends with
/// the HTML block that closes it, which ends at `end`, or else with the
/// last block inside it.
fn details(
    summary: Option<Vec<Inline>>,
    blocks: Vec<(Block, Position)>,
    first: Sourcepos,
    end: Option<comrak::nodes::LineColumn>,
) -> (Block, Position) {
    let (blocks, children): (Vec<_>, Vec<_>) = blocks.into_iter().unzip();
    let block = Block::Details {
        summary: summary.unwrap_or_else(|| vec![Inline::Text("Details".into())]),
        blocks,
    };
    let end = end
        .or_else(|| children.last().map(|last| last.sourcepos().end))
        .unwrap_or(first.end);
    let position = Position {
        at: Some(Found {
            sourcepos: Sourcepos {
                start: first.start,
                end,
            },
            line: first.start.line,
            extent: Extent::Exact,
        }),
        children,
    };
    (block, position)
}

/// The blocks of HTML inside a `<details>` that is not Markdown: what
/// `html_element` reads, or its text as one paragraph.
fn details_body(body: &str) -> Vec<Block> {
    if let Some(block) = html_element(body) {
        return vec![block];
    }
    match html::plain_text(body) {
        Some(text) if !text.is_empty() => vec![Block::Paragraph(vec![Inline::Text(text)])],
        _ => Vec::new(),
    }
}

/// The block that an HTML block of one element is: an image, a heading or
/// a code block.
fn html_element(html: &str) -> Option<Block> {
    if let Some(img) = html::block_img(html) {
        return Some(Block::Paragraph(vec![linked_image(img)]));
    }
    if let Some((level, content)) = html::heading(html) {
        return Some(Block::Heading {
            level,
            content: html_inlines(content),
        });
    }
    let (language, code) = html::pre(html)?;
    Some(Block::CodeBlock {
        language,
        code,
        whole_file: false,
    })
}

// Block types without a model counterpart yet are skipped.
fn block(node: Node<'_>, unknown: &mut Unknown) -> Option<(Block, Position)> {
    let mut children = Vec::new();
    let block = match &node.data().value {
        NodeValue::Heading(heading) => Block::Heading {
            level: heading.level,
            content: inlines(node, unknown),
        },
        NodeValue::Paragraph => Block::Paragraph(inlines(node, unknown)),
        NodeValue::CodeBlock(code) => Block::CodeBlock {
            language: code.info.split_whitespace().next().map(str::to_owned),
            code: code.literal.clone(),
            whole_file: false,
        },
        NodeValue::BlockQuote => {
            let (blocks, positions): (Vec<_>, Vec<_>) = blocks(node, unknown).into_iter().unzip();
            children = positions;
            Block::BlockQuote(blocks)
        }
        NodeValue::Alert(alert) => {
            let (blocks, positions): (Vec<_>, Vec<_>) = blocks(node, unknown).into_iter().unzip();
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
            let (items, positions) = node.children().map(|item| list_item(item, unknown)).unzip();
            children = positions;
            Block::List(List {
                start: (list.list_type == ListType::Ordered).then_some(list.start as u64),
                tight: list.tight,
                items,
            })
        }
        NodeValue::Table(table) => Block::Table(self::table(node, &table.alignments, unknown)),
        NodeValue::ThematicBreak => Block::ThematicBreak,
        NodeValue::HtmlBlock(block) => {
            html_element(&block.literal).unwrap_or_else(|| Block::Html(block.literal.clone()))
        }
        _ => {
            unknown.note(node);
            return None;
        }
    };
    let position = Position {
        children,
        ..Position::exact(node.data().sourcepos)
    };
    Some((block, position))
}

fn list_item(node: Node<'_>, unknown: &mut Unknown) -> (ListItem, Position) {
    let checked = match &node.data().value {
        NodeValue::TaskItem(task) => Some(task.symbol.is_some()),
        _ => None,
    };
    let (blocks, children): (Vec<_>, Vec<_>) = blocks(node, unknown).into_iter().unzip();
    let position = Position { at: None, children };
    (ListItem { checked, blocks }, position)
}

fn table(node: Node<'_>, alignments: &[TableAlignment], unknown: &mut Unknown) -> Table {
    let mut table = Table {
        alignments: alignments.iter().map(|a| alignment(*a)).collect(),
        header: Vec::new(),
        rows: Vec::new(),
    };
    for row in node.children() {
        let is_header = matches!(row.data().value, NodeValue::TableRow(true));
        let cells: Vec<Vec<Inline>> = row.children().map(|cell| inlines(cell, unknown)).collect();
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

fn inlines(parent: Node<'_>, unknown: &mut Unknown) -> Vec<Inline> {
    inlines_of(parent.children(), unknown)
}

fn inlines_of<'a>(nodes: impl Iterator<Item = Node<'a>>, unknown: &mut Unknown) -> Vec<Inline> {
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
        } else if let Some(inline) = inline(node, unknown) {
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
    // Nothing here is Markdown that the parser reads, so nothing is left out.
    let mut unknown = Unknown::default();
    (root.first_child())
        .map(|paragraph| inlines(paragraph, &mut unknown))
        .unwrap_or_default()
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
        // `<code>` is code only around plain text; else both tags stay.
        let code = match phrase {
            html::Phrase::Code => match only_text(&inlines[start + 1..inlines.len() - 1]) {
                Some(code) => code,
                None => return,
            },
            _ => String::new(),
        };
        inlines.pop();
        let content: Vec<Inline> = inlines.drain(start..).skip(1).collect();
        inlines.push(match phrase {
            html::Phrase::Kbd => Inline::Kbd(content),
            html::Phrase::Mark => Inline::Mark(content),
            html::Phrase::Sub => Inline::Subscript(content),
            html::Phrase::Sup => Inline::Superscript(content),
            html::Phrase::B | html::Phrase::Strong => Inline::Strong(content),
            html::Phrase::I | html::Phrase::Em => Inline::Emphasis(content),
            html::Phrase::S | html::Phrase::Del => Inline::Strikethrough(content),
            html::Phrase::Code => Inline::Code(code),
        });
    }
}

/// The text of `inlines` when they are only non-empty text.
fn only_text(inlines: &[Inline]) -> Option<String> {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(part) => text.push_str(part),
            _ => return None,
        }
    }
    (!text.is_empty()).then_some(text)
}

fn inline(node: Node<'_>, unknown: &mut Unknown) -> Option<Inline> {
    Some(match &node.data().value {
        NodeValue::Text(text) => Inline::Text(text.to_string()),
        NodeValue::Code(code) => Inline::Code(code.literal.clone()),
        NodeValue::ShortCode(shortcode) => Inline::Text(shortcode.emoji.clone()),
        NodeValue::Emph => Inline::Emphasis(inlines(node, unknown)),
        NodeValue::Strong => Inline::Strong(inlines(node, unknown)),
        NodeValue::Strikethrough => Inline::Strikethrough(inlines(node, unknown)),
        NodeValue::Link(link) => Inline::Link {
            url: link.url.clone(),
            content: inlines(node, unknown),
        },
        NodeValue::Image(link) => Inline::Image(Image {
            url: link.url.clone(),
            alt: inlines(node, unknown),
            width: None,
            height: None,
        }),
        NodeValue::SoftBreak => Inline::SoftBreak,
        NodeValue::LineBreak => Inline::LineBreak,
        NodeValue::HtmlInline(tag) => html_inline(tag.clone()),
        _ => {
            unknown.note(node);
            return None;
        }
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
