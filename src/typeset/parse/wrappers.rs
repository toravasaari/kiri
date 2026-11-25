//! The HTML elements that wrap blocks of Markdown, joined into one block that holds the blocks between their tags.
//!
//! comrak gives an element with blank lines in it as an HTML block for its
//! start tag, the blocks inside it and an HTML block for its end tag.
//!
//! The elements are `<details>`, and `<p>`, `<div>` and `<center>` that
//! centre what they hold.

use comrak::nodes::Sourcepos;

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
    /// A `<p>` or `<div>` that does not centre, and the HTML block of its
    /// start tag. It is no wrapper: it is there so that its end tag does not
    /// end the element round it, and it stays source as it is.
    Plain(html::Element, (Block, Position)),
}

/// An element still open: what it is, its blocks so far and where its
/// first tag is.
struct Open {
    element: Element,
    blocks: Vec<(Block, Position)>,
    first: Sourcepos,
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
/// that holds more than the tag of an element that centres is no start or
/// end of one: it is read as the block it is.
///
/// An element starts where its first tag does, and the blocks read from
/// the HTML inside a `<details>` start where the HTML block they are in
/// does.
pub(super) fn join_wrappers(
    blocks: impl Iterator<Item = (Block, Position)>,
) -> Vec<(Block, Position)> {
    let mut joiner = Joiner::default();
    for (block, position) in blocks {
        let at = position.sourcepos();
        if let Block::Html(source) = &block {
            if let Some(parts) = html::details_parts(source)
                && reads_details(&parts, joiner.details_open())
            {
                joiner.details_parts(parts, at);
                continue;
            }
            // A `<summary>` after a blank line is still the summary when
            // nothing came before it.
            if let Some(Open {
                element: Element::Details(summary @ None),
                blocks,
                ..
            }) = joiner.open.last_mut()
                && blocks.is_empty()
                && let Some(content) = html::summary(source)
            {
                *summary = Some(html_inlines(content));
                continue;
            }
            match html::edge(source) {
                Some(Edge::Start(element)) => {
                    joiner.open.push(Open {
                        element: Element::Centred(element, (block, position)),
                        blocks: Vec::new(),
                        first: at,
                    });
                    continue;
                }
                Some(Edge::Plain(element)) => {
                    joiner.open.push(Open {
                        element: Element::Plain(element, (block, position)),
                        blocks: Vec::new(),
                        first: at,
                    });
                    continue;
                }
                Some(Edge::End(element)) if joiner.wrapper_on_top(element) => {
                    joiner.end_wrapper(at, (block, position));
                    continue;
                }
                _ => {}
            }
        }
        joiner.push((block, position));
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
        if let Element::Centred(_, start) | Element::Plain(_, start) = open.element {
            self.push(start);
        }
        for block in open.blocks {
            self.push(block);
        }
    }

    /// Whether the element open last is the one `</element>` ends, and it
    /// is a `<p>`, `<div>` or `<center>`.
    fn wrapper_on_top(&self, element: html::Element) -> bool {
        let top = self.open.last();
        matches!(
            top,
            Some(Open { element: Element::Centred(top, _) | Element::Plain(top, _), .. })
                if *top == element
        )
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
    end: Option<comrak::nodes::LineColumn>,
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
    end: Option<comrak::nodes::LineColumn>,
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
