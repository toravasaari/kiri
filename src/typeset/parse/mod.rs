//! Markdown source to document model, through comrak's syntax tree.

pub mod csv;
mod footnotes;
mod front_matter;
mod html;
pub use html::{HtmlLevel, HtmlTags, InnerHeading, anchor_ids};
#[cfg(test)]
mod inline_places;
mod unknown;

use comrak::nodes::{
    AlertType, ListType, NodeMath, NodeTableRow, NodeValue, Sourcepos, TableAlignment,
};
use comrak::{Arena, Node, Options, parse_document};

use self::html::{Joined, Part, html_inlines};
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
    let options = ParseOptions {
        front_matter,
        ..ParseOptions::default()
    };
    parse_with_warnings(source, &options).0
}

/// Parses Markdown at the level "all" of HTML, which reads `<u>` and
/// `<center>` too: for the tests whose subject is what the tables of
/// elements say, not the default level.
#[cfg(test)]
pub fn parse_all(source: &str) -> Document {
    let options = ParseOptions {
        html: HtmlLevel::All,
        ..ParseOptions::default()
    };
    parse_with_warnings(source, &options).0
}

/// What the reading of a document depends on, as the settings give it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Whether front matter at the start is recognised.
    pub front_matter: bool,
    /// The level of HTML that is read: what GitHub shows, or all that kiri
    /// reads.
    pub html: HtmlLevel,
    /// Whether the tags that have no effect are shown dim or left out.
    pub html_tags: HtmlTags,
}

impl Default for ParseOptions {
    /// Front matter recognised and GitHub's level of HTML, as `parse` reads.
    fn default() -> Self {
        Self {
            front_matter: true,
            html: HtmlLevel::default(),
            html_tags: HtmlTags::default(),
        }
    }
}

/// Parses Markdown as [`parse_with`] does with `parse_options`, and tells
/// what it leaves out: a warning for each kind of Markdown element that the
/// document model has no place for, at the line where the first of its kind
/// starts.
pub fn parse_with_warnings(
    source: &str,
    parse_options: &ParseOptions,
) -> (Document, Vec<Diagnostic>) {
    parse_with_options(source, parse_options, &options())
}

fn parse_with_options(
    source: &str,
    parse_options: &ParseOptions,
    options: &Options<'_>,
) -> (Document, Vec<Diagnostic>) {
    let front_matter = parse_options.front_matter;
    let split = front_matter.then(|| front_matter::split(source)).flatten();
    let (front_matter, markdown) = match split {
        Some((front_matter, markdown)) => (Some(Block::FrontMatter(front_matter)), markdown),
        None => (None, source),
    };
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, options);
    let mut unknown = Unknown::new(parse_options.html, parse_options.html_tags);
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
            code: source.into(),
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
    options.extension.footnotes = true;
    options
}

fn blocks(parent: Node<'_>, unknown: &mut Unknown) -> Vec<(Block, Position)> {
    // comrak puts the footnotes last; they are made into one list.
    let (notes, rest): (Vec<_>, Vec<_>) = parent
        .children()
        .partition(|node| matches!(node.data().value, NodeValue::FootnoteDefinition(_)));
    // The HTML blocks and the Markdown blocks between them are read as one
    // tree of HTML.
    let parts = (rest.into_iter())
        .map(|node| match &node.data().value {
            NodeValue::HtmlBlock(block) => Part::Html {
                literal: block.literal.clone(),
                at: node.data().sourcepos,
            },
            _ => Part::Markdown(
                (split_at_formulas(node, unknown))
                    .unwrap_or_else(|| block(node, unknown).into_iter().collect()),
            ),
        })
        .collect();
    let mut blocks = html::read(parts, unknown.html, unknown.html_tags);
    blocks.extend(footnotes::notes(&notes, unknown));
    blocks
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
                code: format!("{source}\n").into(),
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

// Block types without a model counterpart yet are skipped.
fn block(node: Node<'_>, unknown: &mut Unknown) -> Option<(Block, Position)> {
    let mut children = Vec::new();
    let block = match &node.data().value {
        NodeValue::Heading(heading) => {
            let content = inlines(node, unknown);
            if emptied(node, content.is_empty(), unknown) {
                return None;
            }
            Block::Heading {
                level: heading.level,
                content,
                id: None,
            }
        }
        NodeValue::Paragraph => {
            let content = inlines(node, unknown);
            if emptied(node, content.is_empty(), unknown) {
                return None;
            }
            Block::Paragraph(content)
        }
        NodeValue::CodeBlock(code) => Block::CodeBlock {
            language: code.info.split_whitespace().next().map(str::to_owned),
            code: code.literal.as_str().into(),
            whole_file: false,
        },
        NodeValue::BlockQuote => {
            let (blocks, positions): (Vec<_>, Vec<_>) = blocks(node, unknown).into_iter().unzip();
            if emptied(node, blocks.is_empty(), unknown) {
                return None;
            }
            children = positions;
            Block::BlockQuote(blocks)
        }
        NodeValue::Alert(alert) => {
            let (blocks, positions): (Vec<_>, Vec<_>) = blocks(node, unknown).into_iter().unzip();
            if emptied(node, blocks.is_empty(), unknown) {
                return None;
            }
            children = positions;
            Block::Alert {
                kind: match alert.alert_type {
                    AlertType::Note => AlertKind::Note,
                    AlertType::Tip => AlertKind::Tip,
                    AlertType::Important => AlertKind::Important,
                    AlertType::Warning => AlertKind::Warning,
                    AlertType::Caution => AlertKind::Caution,
                    // comrak marks the enum non-exhaustive; its default is Note.
                    _ => AlertKind::Note,
                },
                title: alert.title.clone(),
                blocks,
            }
        }
        NodeValue::List(list) => {
            let (items, positions): (Vec<_>, Vec<_>) = (node.children())
                .filter_map(|item| list_item(item, unknown))
                .unzip();
            if emptied(node, items.is_empty(), unknown) {
                return None;
            }
            children = positions;
            Block::List(List {
                start: (list.list_type == ListType::Ordered).then_some(list.start as u64),
                tight: list.tight,
                footnotes: false,
                items,
            })
        }
        NodeValue::Table(table) => Block::Table(self::table(node, &table.alignments, unknown)),
        NodeValue::ThematicBreak(_) => Block::ThematicBreak,
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

/// Whether the block `node` is left out: hiding the tags emptied it, and it
/// held something as written. What was empty as written stays.
fn emptied(node: Node<'_>, empty: bool, unknown: &Unknown) -> bool {
    empty && unknown.html_tags == HtmlTags::Hide && node.first_child().is_some()
}

/// An item of a list, or `None` when hiding the tags left it with nothing.
fn list_item(node: Node<'_>, unknown: &mut Unknown) -> Option<(ListItem, Position)> {
    let checked = match &node.data().value {
        NodeValue::TaskItem(task) => Some(task.symbol.is_some()),
        _ => None,
    };
    let (blocks, children): (Vec<_>, Vec<_>) = blocks(node, unknown).into_iter().unzip();
    if emptied(node, blocks.is_empty(), unknown) {
        return None;
    }
    let position = Position { at: None, children };
    Some((ListItem { checked, blocks }, position))
}

fn table(node: Node<'_>, alignments: &[TableAlignment], unknown: &mut Unknown) -> Table {
    let (mut header, mut rows) = (Vec::new(), Vec::new());
    for row in node.children() {
        let is_header = matches!(
            row.data().value,
            NodeValue::TableRow(NodeTableRow { header: true, .. })
        );
        let cells: Vec<Vec<Inline>> = row.children().map(|cell| inlines(cell, unknown)).collect();
        if is_header {
            header = cells;
        } else {
            rows.push(cells);
        }
    }
    let alignments = alignments.iter().map(|a| alignment(*a)).collect();
    Table::new(alignments, header, rows)
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
    let mut joined = Joined::new(unknown.html, unknown.html_tags);
    // Whether the last node was math read as text, which the text around
    // it joins.
    let mut after_math = false;
    for node in nodes {
        if let NodeValue::Math(math) = &node.data().value {
            if math.display_math {
                for inline in math_as_written(math) {
                    joined.push_joined(inline);
                }
                after_math = true;
            } else {
                joined.push(Inline::Formula {
                    source: formula_source(&math.literal),
                    written: math_as_written(math),
                });
                after_math = false;
            }
        } else if let NodeValue::HtmlInline(tag) = &node.data().value {
            joined.push_html(tag.clone());
            after_math = false;
        } else if let Some(inline) = inline(node, unknown) {
            match after_math {
                true => joined.push_joined(inline),
                false => joined.push(inline),
            }
            after_math = false;
        }
    }
    joined.into_inlines()
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
    let mut unknown = Unknown::new(HtmlLevel::default(), HtmlTags::default());
    (root.first_child())
        .map(|paragraph| inlines(paragraph, &mut unknown))
        .unwrap_or_default()
}

/// The `<h1>` to `<h6>` elements in the block of HTML `source`, with the
/// tags kiri reads in their content read, as in a heading of its own
/// (see `html::headings`).
pub fn html_headings(source: &str) -> Vec<InnerHeading> {
    let mut found = html::headings(source);
    for heading in &mut found {
        heading.content = html_inlines(
            std::mem::take(&mut heading.content),
            HtmlLevel::default(),
            HtmlTags::default(),
        );
    }
    found
}

fn inline(node: Node<'_>, unknown: &mut Unknown) -> Option<Inline> {
    Some(match &node.data().value {
        NodeValue::Text(text) => Inline::Text(text.to_string()),
        NodeValue::Code(code) => Inline::Code(code.literal.clone()),
        NodeValue::ShortCode(shortcode) => Inline::Text(shortcode.emoji.clone()),
        NodeValue::Emph(_) => Inline::Emphasis(inlines(node, unknown)),
        NodeValue::Strong(_) => Inline::Strong(inlines(node, unknown)),
        NodeValue::Strikethrough(_) => Inline::Strikethrough(inlines(node, unknown)),
        NodeValue::Link(link) => Inline::Link {
            url: link.url.clone(),
            title: Some(link.title.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|title| !title.is_empty()),
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
        NodeValue::FootnoteReference(reference) => footnotes::reference(reference),
        _ => {
            unknown.note(node);
            return None;
        }
    })
}

#[cfg(test)]
mod tests;
