//! The HTML elements that wrap blocks of Markdown, joined into one block that holds the blocks between their tags.
//!
//! comrak gives an element with blank lines in it as an HTML block for its
//! start tag, the blocks inside it and an HTML block for its end tag.
//!
//! The elements are `<details>`, and `<p>`, `<div>` and `<center>` that
//! centre what they hold.

use comrak::nodes::{LineColumn, Sourcepos};

use super::html::{self, DetailsPart, Edge, Part};
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

/// An element that `join_wrappers` has seen the start of, and not the end.
enum Element {
    /// A `<details>` and its summary.
    Details(Option<Vec<Inline>>),
    /// A `<p>`, `<div>` or `<center>` that centres, and the HTML block of
    /// its start tag, which it stays when nothing ends it.
    Centred(html::Element, (Block, Position)),
    /// A `<p>` or `<div>` that does not centre, or that has more than its
    /// start tag in its HTML block. It is no wrapper: it is there so that
    /// its end tag does not end the element round it. Its start tag is a
    /// block of the element round it, and it stays source as it is.
    Plain(html::Element),
}

/// An element still open: what it is, its blocks so far and where its
/// first tag is.
struct Open {
    element: Element,
    blocks: Vec<(Block, Position)>,
    first: Sourcepos,
}

impl Open {
    /// Whether it is a `<p>`, `<div>` or `<center>` that `</element>` ends.
    fn is(&self, element: html::Element) -> bool {
        matches!(
            &self.element,
            Element::Centred(open, _) | Element::Plain(open) if *open == element
        )
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
/// `<div>` or `<center>`, in them as one block. comrak gives such an
/// element with blank lines in it as an HTML block that starts it, the
/// blocks inside and an HTML block that ends it. A `<details>` that never
/// ends runs to the end of `blocks`, as in a browser, and its end tag ends
/// the elements open inside it, which stay source. An element that
/// centres and never ends stays what it is, its start tag as an HTML
/// block and the blocks after it as they are, and so does an end tag that
/// does not end the element open last.
///
/// comrak ends an HTML block only at a blank line, so a block can hold
/// more than a `<details>` with its `<summary>`, or more than a
/// `</details>`. A block that starts with a `<details>`, or that holds a
/// `</details>` while one is open, is read tag by tag; the HTML between
/// its tags is read as the body of a `<details>` in one block is. A block
/// that holds more than the tag of an element that centres is no start of
/// one: it is read as the block it is. One that starts with the end tag of
/// the element open last ends it there, and what follows the tag is read
/// as a block after the element, which may start with end tags or a start
/// tag again. The `<p>` and `<div>` start tags in a block that it does not
/// end are still open after it, so that their end tags do not end the
/// element round them.
///
/// A `<p>` that does not centre ends as in a browser: at the start tag of
/// a `<p>`, `<div>` or `<center>`, and at the end tag of the element round
/// it. It stays source.
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

    /// Whether a `<details>` is open, with elements that centre or not
    /// above it or not.
    fn details_open(&self) -> bool {
        (self.open.iter()).any(|open| matches!(open.element, Element::Details(_)))
    }

    /// Ends the elements above the `<details>` open last, as a browser
    /// does at its end tag: they stay source, as `finish` leaves them.
    fn dissolve_above_details(&mut self) {
        while let Some(open) = self
            .open
            .pop_if(|open| !matches!(open.element, Element::Details(_)))
        {
            self.dissolve(open);
        }
    }

    /// The element `open`, which is no `<details>`, as its start tag and
    /// the blocks it holds.
    fn dissolve(&mut self, open: Open) {
        if let Element::Centred(_, start) = open.element {
            self.push(start);
        }
        for block in open.blocks {
            self.push(block);
        }
    }

    /// Whether the element open last is the one `</element>` ends, and it
    /// is a `<p>`, `<div>` or `<center>`.
    fn wrapper_on_top(&self, element: html::Element) -> bool {
        self.open.last().is_some_and(|open| open.is(element))
    }

    /// Whether `</element>` ends the element open last, once the elements
    /// that a browser ends before it are ended. `</p>` ends a `<p>` only
    /// when one is open, and stays source else.
    fn ends_on_top(&mut self, element: html::Element) -> bool {
        self.end_open_paragraphs(Some(element));
        self.wrapper_on_top(element)
    }

    /// Ends the `<p>` elements that do not centre and are open last, as a
    /// browser ends a `<p>` without its end tag: at the start of a `<p>`,
    /// `<div>` or `<center>`, when `ending` is `None`, and at the end tag of
    /// the element `ending` that is open below them. They stay source.
    fn end_open_paragraphs(&mut self, ending: Option<html::Element>) {
        let above = (self.open.iter().rev())
            .take_while(|open| matches!(open.element, Element::Plain(html::Element::P)))
            .count();
        if let Some(element) = ending {
            let below = self.open.iter().rev().nth(above);
            if element == html::Element::P || !below.is_some_and(|open| open.is(element)) {
                return;
            }
        }
        for open in self.open.split_off(self.open.len() - above) {
            self.dissolve(open);
        }
    }

    /// The `<p>` and `<div>` elements that the HTML block `block` starts
    /// and does not end: it is added as it is, and they are open after it.
    fn open_plain(&mut self, block: (Block, Position), open: Vec<html::Element>, first: Sourcepos) {
        self.push(block);
        for element in open {
            self.end_open_paragraphs(None);
            self.open.push(Open {
                element: Element::Plain(element),
                blocks: Vec::new(),
                first,
            });
        }
    }

    /// An HTML block `block`, whose source is `source`, read tag by tag.
    ///
    /// A block that is a start or an end tag alone, or that is a `<details>`
    /// or `<summary>`, is that. A block that starts with an end tag that
    /// ends the element open last is cut after it, and what follows is read
    /// the same way as a block of its own: as the next end tags, a start tag
    /// or the rest of the block. What is left stays as it is, a block that
    /// opens the `<p>` and `<div>` elements it starts and does not end.
    fn html(&mut self, block: (Block, Position), source: &str) {
        let whole = block.1.sourcepos();
        let mut block = Some(block);
        let mut from = 0;
        loop {
            let piece = &source[from..];
            let at = Sourcepos {
                start: line_column(whole, source, from),
                end: whole.end,
            };
            // The piece as one block of HTML: the block as it came, while
            // nothing is cut off it.
            let mut this = |piece: &str| {
                block.take().unwrap_or_else(|| {
                    (
                        Block::Html(piece.to_owned()),
                        Position::inside(whole, at.start.line),
                    )
                })
            };
            if let Some(parts) = html::details_parts(piece)
                && reads_details(&parts, self.details_open())
            {
                self.details_parts(parts, at);
                return;
            }
            // A `<summary>` after a blank line is still the summary when
            // nothing came before it.
            if let Some(Open {
                element: Element::Details(summary @ None),
                blocks,
                ..
            }) = self.open.last_mut()
                && blocks.is_empty()
                && let Some(content) = html::summary(piece)
            {
                *summary = Some(html_inlines(content));
                return;
            }
            match html::edge(piece) {
                Some(Edge::Start(element)) => {
                    self.end_open_paragraphs(None);
                    self.open.push(Open {
                        element: Element::Centred(element, this(piece)),
                        blocks: Vec::new(),
                        first: at,
                    });
                    return;
                }
                Some(Edge::End(element)) if self.ends_on_top(element) => {
                    self.end_wrapper(at, this(piece));
                    return;
                }
                Some(Edge::End(_)) => {}
                None => {
                    if let Some((element, end)) = html::leading_end(piece)
                        && self.ends_on_top(element)
                    {
                        let tag_end = line_column(whole, source, from + end - 1);
                        let tag = (
                            Block::Html(piece[..end].to_owned()),
                            Position::inside(whole, at.start.line),
                        );
                        self.end_wrapper(Sourcepos { end: tag_end, ..at }, tag);
                        let rest = &piece[end..];
                        from += end + rest.len() - rest.trim_start().len();
                        block = None;
                        continue;
                    }
                }
            }
            let block = match this(piece) {
                (Block::Html(_), position) if from > 0 => (rest_block(piece), position),
                block => block,
            };
            self.open_plain(block, html::open_elements(piece), at);
            return;
        }
    }

    /// The end of the `<p>`, `<div>` or `<center>` open last, in the HTML
    /// block `end` that ends at `at`: the element that centres becomes one
    /// block, and one that does not stays its tags and the blocks between.
    fn end_wrapper(&mut self, at: Sourcepos, end: (Block, Position)) {
        let open = self.open.pop().expect("an element is open");
        match open.element {
            Element::Centred(..) => {
                let block = wrapped(Block::Centered, open.blocks, open.first, Some(at.end));
                self.push(block);
            }
            Element::Plain(..) => {
                self.dissolve(open);
                self.push(end);
            }
            Element::Details(_) => unreachable!("a `<p>`, `<div>` or `<center>` is on top"),
        }
    }

    /// The tags of the HTML block at `at`, cut into `parts`.
    fn details_parts(&mut self, parts: Vec<DetailsPart<'_>>, at: Sourcepos) {
        for part in parts {
            match part {
                DetailsPart::Start(summary) => self.open.push(Open {
                    element: Element::Details(summary.map(html_inlines)),
                    blocks: Vec::new(),
                    first: at,
                }),
                DetailsPart::End(_) if self.details_open() => {
                    self.dissolve_above_details();
                    let open = self.open.pop().expect("a `<details>` is open");
                    let Element::Details(summary) = open.element else {
                        unreachable!("a `<details>` is on top");
                    };
                    let block = details(summary, open.blocks, open.first, Some(at.end));
                    self.push(block);
                }
                // A `</details>` that ends no `<details>` stays source.
                DetailsPart::End(tag) => self.push((
                    Block::Html(tag.to_owned()),
                    Position::inside(at, at.start.line),
                )),
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
            match open.element {
                Element::Details(summary) => {
                    self.push(details(summary, open.blocks, open.first, None));
                }
                Element::Centred(..) | Element::Plain(..) => self.dissolve(open),
            }
        }
        self.joined
    }
}

/// Whether `join_wrappers` reads an HTML block of `parts` tag by tag: when
/// it starts with a `<details>`, or holds a `</details>` and one is `open`,
/// whatever is open above it.
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
    end: Option<LineColumn>,
) -> (Block, Position) {
    let summary = summary.unwrap_or_else(|| vec![Inline::Text("Details".into())]);
    wrapped(
        |blocks| Block::Details { summary, blocks },
        blocks,
        first,
        end,
    )
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
