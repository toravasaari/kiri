//! The HTML elements that wrap blocks of Markdown, joined into one block that holds the blocks between their tags.
//!
//! comrak gives an element with blank lines in it as an HTML block for its
//! start tag, the blocks inside it and an HTML block for its end tag.
//!
//! The elements are `<details>`, and `<p>`, `<div>` and `<center>` that
//! centre what they hold.

use comrak::nodes::{LineColumn, Sourcepos};

use super::html::{self, DetailsPart, Element, Event, Mark, Part};
use super::{Block, Extent, Found, Inline, Position, html_element, html_inlines, linked_image};

/// The block that an HTML block of one element that centres is, and the
/// places of the blocks in it, which are read from `whole`, the place of
/// the HTML block. `None` for any other HTML.
pub(super) fn centred(html: &str, whole: Sourcepos) -> Option<(Block, Vec<Position>)> {
    let (blocks, positions) = html::centred(html)?
        .into_iter()
        .map(|piece| {
            let block = match piece.part {
                Part::Heading(level, content, id) => Block::Heading {
                    level,
                    content: html_inlines(content),
                    id,
                },
                Part::Paragraph(content) => Block::Paragraph(html_inlines(content)),
                Part::Image(img) => Block::Paragraph(vec![linked_image(img)]),
            };
            let line = whole.start.line + html[..piece.at].matches('\n').count();
            (block, Position::inside(whole, line))
        })
        .unzip();
    Some((Block::Centered(blocks), positions))
}

/// What an open element becomes when it ends.
enum Kind {
    /// A `<details>` and its summary.
    Details(Option<Vec<Inline>>),
    /// An element that centres, and the HTML block of its start tag, which
    /// it stays when nothing ends it.
    Centred((Block, Position)),
    /// An element that does not centre, or whose start tag shares its HTML
    /// block with more. It is no wrapper: it is there so that its end tag
    /// does not end the element round it. Its start tag is a block of the
    /// element round it, and it stays source as it is.
    Plain,
}

/// An element still open: what it is, its blocks so far and where its
/// first tag is.
struct Open {
    element: &'static Element,
    kind: Kind,
    blocks: Vec<(Block, Position)>,
    first: Sourcepos,
}

impl Open {
    fn new(element: &'static Element, kind: Kind, first: Sourcepos) -> Self {
        Self {
            element,
            kind,
            blocks: Vec::new(),
            first,
        }
    }

    /// Whether a browser ends it without its end tag. A `<p>` that centres
    /// does not end so here: it nests in the `<p>` round it.
    fn ends_implied(&self) -> bool {
        self.element.implied_end && matches!(self.kind, Kind::Plain)
    }
}

/// The line and the column in the document of the byte `byte` of `source`,
/// the text of the HTML block that is at `whole`.
fn line_column(whole: Sourcepos, source: &str, byte: usize) -> LineColumn {
    let before = &source[..byte];
    match before.rfind('\n') {
        Some(newline) => LineColumn {
            line: whole.start.line + before.matches('\n').count(),
            column: whole.start.column.saturating_sub(1) + byte - newline,
        },
        None => LineColumn {
            line: whole.start.line,
            column: whole.start.column + byte,
        },
    }
}

/// The block that the HTML `rest`, what follows an end tag in its block,
/// is: what `html_element` reads, text as a paragraph, or HTML.
fn rest_block(rest: &str) -> Block {
    match html_element(rest) {
        Some(block) => block,
        None if !rest.contains('<') => match html::plain_text(rest) {
            Some(text) => Block::Paragraph(vec![Inline::Text(text)]),
            None => Block::Html(rest.to_owned()),
        },
        None => Block::Html(rest.to_owned()),
    }
}

/// `blocks` with each `<details>` and each element that centres, `<p>`,
/// `<div>` or `<center>`, that blank lines split in them as one block.
///
/// The tags of each HTML block are read in order and kept on a stack of
/// open elements, as a browser keeps them, and a block of Markdown, or one
/// read whole before (`centred`, `html_element`), is a block of the element
/// open last. Where the HTML is broken, the tags stay source:
///
/// - A start tag that centres starts a wrapper only when it is alone in
///   its HTML block; with more beside it, it opens a plain element.
/// - An end tag that ends no open element stays source; so does one that
///   meets an element above the one it would end, but for a `<p>` that
///   does not centre, which ends as in a browser, and stays source.
/// - An element that never ends stays source: its start tag as an HTML
///   block and the blocks after it as they are. A `<details>` that never
///   ends runs to the end of `blocks`, as in a browser, and its end tag
///   ends the elements open inside it, which stay source.
/// - An HTML block is cut only after the end tags it starts with. An end
///   tag after something else in its block ends only what the block
///   started; what follows a cut is read as a block after the element.
/// - A block that starts with a `<details>`, or that holds a `</details>`
///   while one is open, is read tag by tag, and the HTML between those
///   tags is read as the body of a `<details>` in one block is.
/// - A `<summary>` counts right after its `<details>`, or alone in the next
///   block while the `<details>` holds nothing yet.
/// - A block whose tags cannot be read is a block as it is.
///
/// An element starts where its first tag does, and the blocks read from
/// the HTML inside a `<details>` start where the HTML block they are in
/// does.
pub(super) fn join_wrappers(
    blocks: impl Iterator<Item = (Block, Position)>,
) -> Vec<(Block, Position)> {
    let mut joiner = Joiner::default();
    for (block, position) in blocks {
        match &block {
            Block::Html(source) => {
                let source = source.clone();
                joiner.html((block, position), &source);
            }
            _ => joiner.push((block, position)),
        }
    }
    joiner.finish()
}

/// The blocks of `join_wrappers` so far.
#[derive(Default)]
struct Joiner {
    joined: Vec<(Block, Position)>,
    open: Vec<Open>,
}

impl Joiner {
    /// Adds `block` to the element open last, or else to the blocks.
    fn push(&mut self, block: (Block, Position)) {
        match self.open.last_mut() {
            Some(open) => open.blocks.push(block),
            None => self.joined.push(block),
        }
    }

    /// Whether a `<details>` is open, with other elements above it or not.
    fn details_open(&self) -> bool {
        (self.open.iter()).any(|open| matches!(open.kind, Kind::Details(_)))
    }

    /// Opens `open` at its start tag, after the `<p>` that it ends.
    fn start(&mut self, open: Open) {
        if open.element.ends_p {
            while let Some(open) = self.open.pop_if(|open| open.ends_implied()) {
                self.dissolve(open);
            }
        }
        self.open.push(open);
    }

    /// Where on the stack the open element is that an end tag of `element`
    /// ends: the one open last, when only elements that end with it are
    /// open above it. `None` when it ends nothing, and stays source.
    fn ending(&self, element: &Element) -> Option<usize> {
        for (at, open) in self.open.iter().enumerate().rev() {
            if open.element == element {
                return Some(at);
            }
            if !element.ends_inside && !open.ends_implied() {
                return None;
            }
        }
        None
    }

    /// Ends the open element at `at`, whose end tag ends at `end` and is
    /// the block `tag`, and the elements above it, which stay source. One
    /// that centres becomes one block, and one that does not stays its tags
    /// and the blocks between.
    fn end(&mut self, at: usize, end: LineColumn, tag: Option<(Block, Position)>) {
        while self.open.len() > at + 1 {
            let open = self.open.pop().expect("an element is open");
            self.dissolve(open);
        }
        let open = self.open.pop().expect("an element is open");
        match open.kind {
            Kind::Details(summary) => {
                self.push(details(summary, open.blocks, open.first, Some(end)));
            }
            Kind::Centred(_) => {
                let block = wrapped(Block::Centered, open.blocks, open.first, Some(end));
                self.push(block);
            }
            Kind::Plain => {
                self.dissolve(open);
                self.push(tag.expect("a plain element ends at a tag of its own"));
            }
        }
    }

    /// The element `open`, which is no `<details>`, as its start tag and
    /// the blocks it holds.
    fn dissolve(&mut self, open: Open) {
        if let Kind::Centred(start) = open.kind {
            self.push(start);
        }
        for block in open.blocks {
            self.push(block);
        }
    }

    /// An HTML block `block`, whose source is `source`, read tag by tag.
    ///
    /// A block that is a start tag that centres, alone, starts a wrapper. A
    /// block that starts with an end tag that ends an open element is cut
    /// after it, and what follows is read the same way as a block of its
    /// own. What is left stays as it is, a block that opens the elements it
    /// starts and does not end.
    fn html(&mut self, block: (Block, Position), source: &str) {
        let whole = block.1.sourcepos();
        let Some(events) = html::events(source) else {
            return self.push(block);
        };
        let mut block = Some(block);
        // The byte and the event where what is left of the block starts.
        let (mut from, mut next) = (0, 0);
        loop {
            let rest = &events[next..];
            let at = Sourcepos {
                start: line_column(whole, source, from),
                end: whole.end,
            };
            let inside = || Position::inside(whole, at.start.line);
            // What is left as one block: the block as it came, while
            // nothing is cut off it.
            let this =
                (block.take()).unwrap_or_else(|| (Block::Html(source[from..].into()), inside()));
            if let Some(parts) = html::details_parts(source, from, rest)
                && reads_details(&parts, self.details_open())
            {
                return self.details_parts(parts, at);
            }
            if self.summary(rest) {
                return;
            }
            let first = rest.first().map(|event| (&event.mark, event.span.end));
            if let (Some((&Mark::Start(element, true), _)), 1) = (first, rest.len()) {
                return self.start(Open::new(element, Kind::Centred(this), at));
            }
            if let Some((&Mark::End(element), tag_end)) = first
                && let Some(open) = self.ending(element)
            {
                if rest.len() == 1 {
                    return self.end(open, at.end, Some(this));
                }
                let tag = (Block::Html(source[from..tag_end].into()), inside());
                self.end(open, line_column(whole, source, tag_end - 1), Some(tag));
                let after = &source[tag_end..];
                from = tag_end + after.len() - after.trim_start().len();
                next += 1;
                continue;
            }
            self.push(match this {
                (Block::Html(_), position) if from > 0 => (rest_block(&source[from..]), position),
                block => block,
            });
            for element in left_open(rest) {
                self.start(Open::new(element, Kind::Plain, at));
            }
            return;
        }
    }

    /// Whether the events `rest` are a `<summary>` alone in its block,
    /// which is taken as the summary of the `<details>` open last when that
    /// has no summary and no blocks yet.
    fn summary(&mut self, rest: &[Event]) -> bool {
        let Some(Mark::Summary(Some((content, end)))) = rest.first().map(|event| &event.mark)
        else {
            return false;
        };
        if let Some(Open {
            kind: Kind::Details(summary @ None),
            blocks,
            ..
        }) = self.open.last_mut()
            && blocks.is_empty()
            && rest.iter().all(|event| event.span.end <= *end)
        {
            *summary = Some(html_inlines(content.clone()));
            return true;
        }
        false
    }

    /// The tags of the HTML block at `at`, cut into `parts`.
    fn details_parts(&mut self, parts: Vec<DetailsPart<'_>>, at: Sourcepos) {
        for part in parts {
            match part {
                DetailsPart::Start(element, summary) => {
                    let kind = Kind::Details(summary.map(html_inlines));
                    self.start(Open::new(element, kind, at));
                }
                DetailsPart::End(element, tag) => match self.ending(element) {
                    Some(open) => self.end(open, at.end, None),
                    // A `</details>` that ends no `<details>` stays source.
                    None => self.push((
                        Block::Html(tag.to_owned()),
                        Position::inside(at, at.start.line),
                    )),
                },
                DetailsPart::Html(body) => {
                    for block in details_body(body) {
                        self.push((block, Position::inside(at, at.start.line)));
                    }
                }
            }
        }
    }

    /// The blocks, with the elements still open ended: a `<details>` runs
    /// to the last block, and an element that centres stays its start tag
    /// and the blocks after it.
    fn finish(mut self) -> Vec<(Block, Position)> {
        while let Some(open) = self.open.pop() {
            match open.kind {
                Kind::Details(summary) => {
                    self.push(details(summary, open.blocks, open.first, None));
                }
                Kind::Centred(_) | Kind::Plain => self.dissolve(open),
            }
        }
        self.joined
    }
}

/// The elements that the events `events` of an HTML block start and do not
/// end, of those that open beside other things. An end tag ends the element
/// of its name last started among them, and those started after it; one
/// that matches none is left out.
fn left_open(events: &[Event]) -> Vec<&'static Element> {
    let mut open: Vec<&'static Element> = Vec::new();
    for event in events {
        match event.mark {
            Mark::Start(element, _) if element.opens_beside => open.push(element),
            Mark::End(element) => {
                if let Some(at) = open.iter().rposition(|open| *open == element) {
                    open.truncate(at);
                }
            }
            _ => {}
        }
    }
    open
}

/// Whether `join_wrappers` reads an HTML block of `parts` tag by tag: when
/// it starts with a `<details>`, or holds a `</details>` and one is `open`,
/// whatever is open above it.
fn reads_details(parts: &[DetailsPart], open: bool) -> bool {
    match parts {
        [DetailsPart::Start(..), ..] => true,
        _ => open && (parts.iter()).any(|part| matches!(part, DetailsPart::End(..))),
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
    end: Option<LineColumn>,
) -> (Block, Position) {
    let summary = summary.unwrap_or_else(|| vec![Inline::Text("Details".into())]);
    let make = |blocks| Block::Details { summary, blocks };
    wrapped(make, blocks, first, end)
}

/// The block that `make` gives for `blocks`, with the place that goes from
/// the start of `first` to `end`, or else to the end of the last of them.
fn wrapped(
    make: impl FnOnce(Vec<Block>) -> Block,
    blocks: Vec<(Block, Position)>,
    first: Sourcepos,
    end: Option<LineColumn>,
) -> (Block, Position) {
    let (blocks, children): (Vec<_>, Vec<_>) = blocks.into_iter().unzip();
    let block = make(blocks);
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

#[cfg(test)]
mod tests;
