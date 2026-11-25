//! Markdown source to document model, through comrak's syntax tree.

pub mod csv;
mod footnotes;
mod front_matter;
mod html;
pub use html::{InnerHeading, anchor_ids};
#[cfg(test)]
mod inline_places;
mod unknown;
mod wrappers;

use comrak::nodes::{
    AlertType, ListType, NodeMath, NodeTableRow, NodeValue, Sourcepos, TableAlignment,
};
use comrak::{Arena, Node, Options, parse_document};

use self::unknown::Unknown;
use self::wrappers::{centred, join_wrappers, starts_element};
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
    let mut blocks =
        join_wrappers(
            rest.into_iter()
                .flat_map(|node| match split_at_formulas(node, unknown) {
                    Some(blocks) => blocks,
                    None => block(node, unknown).into_iter().collect(),
                }),
        );
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

/// The block that an HTML block of one element is: an image, a heading or
/// a code block.
fn html_element(html: &str) -> Option<Block> {
    if let Some(img) = html::block_img(html) {
        return Some(Block::Paragraph(linked_image(img)));
    }
    if let Some((level, content, id)) = html::heading(html) {
        return Some(Block::Heading {
            level,
            content: html_inlines(content),
            id,
        });
    }
    let (language, code) = html::pre(html)?;
    Some(Block::CodeBlock {
        language,
        code: code.into(),
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
            id: None,
        },
        NodeValue::Paragraph => Block::Paragraph(inlines(node, unknown)),
        NodeValue::CodeBlock(code) => Block::CodeBlock {
            language: code.info.split_whitespace().next().map(str::to_owned),
            code: code.literal.as_str().into(),
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
                    // comrak marks the enum non-exhaustive; its default is Note.
                    _ => AlertKind::Note,
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
                footnotes: false,
                items,
            })
        }
        NodeValue::Table(table) => Block::Table(self::table(node, &table.alignments, unknown)),
        NodeValue::ThematicBreak(_) => Block::ThematicBreak,
        // Its start tag may end an open `<p>`: `join_wrappers` reads it.
        NodeValue::HtmlBlock(block) if starts_element(&block.literal) => {
            Block::Html(block.literal.clone())
        }
        NodeValue::HtmlBlock(block) => match centred(&block.literal, node.data().sourcepos) {
            Some((block, inside)) => {
                children = inside;
                block
            }
            None => {
                html_element(&block.literal).unwrap_or_else(|| Block::Html(block.literal.clone()))
            }
        },
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

/// The `<h1>` to `<h6>` elements in the block of HTML `source`, with the
/// tags kiri reads in their content read, as in a heading of its own
/// (see `html::headings`).
pub fn html_headings(source: &str) -> Vec<InnerHeading> {
    let mut found = html::headings(source);
    for heading in &mut found {
        heading.content = html_inlines(std::mem::take(&mut heading.content));
    }
    found
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
    join_image_link(inlines);
    join_phrase(inlines);
}

/// `<a href>`, an `<img>` and `</a>` at the end of `inlines` as a link
/// around the image (`named_link`).
fn join_image_link(inlines: &mut Vec<Inline>) {
    // `<a href>`, an `<img>` and `</a>` are a link around the image, as
    // `[![]()]()` is.
    if let [.., Inline::Html(start), Inline::Image(_), Inline::Html(end)] = inlines.as_slice()
        && html::is_link_end(end)
        && let Some(url) = html::link_start(start)
    {
        inlines.pop();
        let image = inlines.pop().expect("the image");
        let Some(Inline::Html(start)) = inlines.pop() else {
            unreachable!("the start tag")
        };
        named_link(inlines, anchor_ids(&start), Some(url), vec![image]);
    }
}

/// Adds to `inlines` the places `ids` that an `<a>` names, as anchors, and
/// `content`, in a link to `url` when the `<a>` has an `href`.
fn named_link(
    inlines: &mut Vec<Inline>,
    ids: Vec<String>,
    url: Option<String>,
    content: Vec<Inline>,
) {
    inlines.extend(ids.into_iter().map(Inline::Anchor));
    match url {
        Some(url) => inlines.push(Inline::Link {
            url,
            title: None,
            content,
        }),
        None => inlines.extend(content),
    }
}

/// A closing tag such as `</kbd>` at the end of `inlines` ends the element
/// its last opening tag among them started: the two tags and what is
/// between them become the element. A `</a>` is ended by the last `<a>`
/// among them, one without an `href` as well, which makes no link and stays
/// HTML, so that it does not end the link before it.
fn join_phrase(inlines: &mut Vec<Inline>) {
    if let Some(Inline::Html(end)) = inlines.last()
        && let Some(phrase) = html::phrase_end(end)
        && let Some(start) = inlines.iter().rposition(|inline| {
            matches!(inline, Inline::Html(start)
                if html::phrase_start(start) == Some(phrase)
                    || (phrase == html::Phrase::A && html::is_anchor_start(start)))
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
        // A link takes the address of its start tag, and has to hold
        // something.
        let link = match (phrase, &inlines[start]) {
            (html::Phrase::A, Inline::Html(tag)) => html::link_start(tag).map(|url| (tag, url)),
            _ => None,
        };
        if phrase == html::Phrase::A && (link.is_none() || start + 2 == inlines.len()) {
            return;
        }
        let link = link.map(|(tag, url)| (tag.clone(), url));
        inlines.pop();
        let content: Vec<Inline> = inlines.drain(start..).skip(1).collect();
        if let Some((tag, url)) = link {
            return named_link(inlines, anchor_ids(&tag), Some(url), content);
        }
        inlines.push(match phrase {
            html::Phrase::A => unreachable!("a link is made above"),
            html::Phrase::U => Inline::Underline(content),
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
        NodeValue::HtmlInline(tag) => html_inline(tag.clone()),
        NodeValue::FootnoteReference(reference) => footnotes::reference(reference),
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
/// it when the HTML had one, after the places that the `<a>` names.
fn linked_image(mut img: html::Img) -> Vec<Inline> {
    let (url, ids) = (img.link.take(), std::mem::take(&mut img.ids));
    let mut inlines = Vec::new();
    named_link(&mut inlines, ids, url, vec![image(img)]);
    inlines
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
