//! The tree of the block elements in the HTML blocks of a document and the Markdown blocks between them, built once from their tokens.
//!
//! The pass goes over a container (the document, a quote, a list item), over
//! every token of every HTML block in order, and each Markdown block as a
//! leaf. Only block elements nest; text, and the tags of text elements and of
//! elements kiri does not know, are leaves that the inline join reads
//! later. Nothing is repaired and nothing is moved: an element is ended, or
//! marked unpaired, by setting a field, so that every node keeps the HTML
//! block it is in and its bytes there.
//!
//! The rules, for a stack of open elements:
//!
//! 1. A start tag first ends the open elements that its row says it ends
//!    (a `<p>` by the start of any block), then opens its own element.
//! 2. An end tag ends the nearest open element of its name. The elements
//!    open above that one end with it: one whose end tag may be left out is
//!    whole, and any other becomes an unpaired start tag, but only when no
//!    end tag of its own is left for it. When one is, the end tag at hand is
//!    the one too many, and is an unpaired tag, a leaf. So is an end tag with
//!    no element of its name open.
//! 3. A Markdown block is a leaf of the element open last, after it ends an
//!    open `<p>`.
//! 4. At the end of the container, an element still open whose end tag may
//!    not be left out is an unpaired start tag.
//!
//! "Left for it" is counted by name from the tags still to come in the
//! container, so an end tag costs the number of kinds of element, not the
//! depth of the stack.

use std::ops::Range;

use super::elements::{self, Element, EndedBy, Kind, TABLE, Treatment};
use super::{Tag, Token};

/// How many block elements may be open at once. A start tag past that is an
/// unpaired tag, so every walk of the tree is bounded.
pub const MAX_DEPTH: usize = 256;

/// A part of a container, as the tree is made from it.
pub enum Piece<'a> {
    /// The tokens of one HTML block, each with its bytes in the block.
    Html(Vec<(Token<'a>, Range<usize>)>),
    /// An HTML block whose tags cannot be read: one leaf, as it is.
    Source,
    /// A block of Markdown.
    Markdown,
}

/// Where a node is: the piece of the container it is in (counted from 0 in
/// the order given) and its bytes in that HTML block; none for a block of
/// Markdown or of source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub piece: usize,
    pub bytes: Range<usize>,
}

/// How an element ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Closing {
    /// By its end tag, which is here.
    End(Span),
    /// Without one, as its row allows: a browser ends it, at the end tag of
    /// the element round it, at a start tag, at a block of Markdown or at the
    /// end of the container. The element is whole.
    Implied,
    /// Never ended, and its end tag may not be left out: its start tag is an
    /// unpaired tag, and what it holds belongs to the element round it.
    Unpaired,
}

/// An open block element.
#[derive(Debug)]
pub struct Opened {
    pub row: &'static Element,
    /// Its start tag, with its attributes.
    pub tag: Tag,
    pub closing: Closing,
}

#[derive(Debug)]
pub enum NodeKind {
    /// A block element, and the nodes after it that have it as their parent.
    Element(Opened),
    /// Text, or what a raw-text element holds.
    Text,
    /// A tag of a text element or of an element that kiri does not know.
    Tag {
        name: String,
        end: bool,
    },
    Comment,
    /// A block of Markdown.
    Markdown,
    /// An HTML block that could not be read.
    Source,
    /// A tag of a block element that starts or ends nothing: a start tag past
    /// the depth cap, an end tag with no element of its name open, or one that
    /// would end an element with an end tag of its own to come.
    Unpaired {
        end: bool,
    },
}

#[derive(Debug)]
pub struct Node {
    /// The element it is in, or none when it is in the container itself.
    pub parent: Option<usize>,
    pub at: Span,
    pub kind: NodeKind,
}

/// The nodes of a container in the order read, each with its parent.
#[derive(Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
}

/// Whether the elements of the row nest: a block or a raw-text element.
fn nests(row: &Element) -> bool {
    matches!(row.kind, Kind::Block | Kind::RawText)
}

/// Whether the start tag of the row starts a block in a browser's sense,
/// which ends an open `<p>`: a block element or a `<pre>`, not a `<script>`,
/// a `<style>` or a `<textarea>`.
fn starts_block(row: &Element) -> bool {
    matches!(row.kind, Kind::Block | Kind::VoidBlock) || row.treatment == Treatment::Pre
}

/// How an element ends that something else ends: whole when its end tag may
/// be left out, else as an unpaired start tag.
fn given_up(row: &Element) -> Closing {
    match row.end_optional {
        true => Closing::Implied,
        false => Closing::Unpaired,
    }
}

/// The tags of the block elements in `pieces` still to come, by element.
struct Counts {
    starts: Vec<usize>,
    ends: Vec<usize>,
}

impl Counts {
    /// Counts the start and end tags of block elements by name in one pass,
    /// not those inside a raw-text element.
    fn of(pieces: &[Piece<'_>]) -> Self {
        let mut counts = Self {
            starts: vec![0; TABLE.len()],
            ends: vec![0; TABLE.len()],
        };
        let mut raw = None;
        for piece in pieces {
            let Piece::Html(tokens) = piece else { continue };
            for (token, _) in tokens {
                match (token, raw) {
                    (Token::Close(name), Some(inside)) => {
                        if elements::index(name) == Some(inside) {
                            counts.ends[inside] += 1;
                            raw = None;
                        }
                    }
                    (_, Some(_)) => {}
                    (Token::Open(tag), None) => {
                        if let Some(at) = elements::index(&tag.name)
                            && nests(&TABLE[at])
                        {
                            counts.starts[at] += 1;
                            if TABLE[at].kind == Kind::RawText {
                                raw = Some(at);
                            }
                        }
                    }
                    (Token::Close(name), None) => {
                        if let Some(at) = elements::index(name)
                            && nests(&TABLE[at])
                        {
                            counts.ends[at] += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        counts
    }
}

struct Builder {
    nodes: Vec<Node>,
    /// The open elements, as the nodes they are, from the outermost.
    stack: Vec<usize>,
    /// The open elements of each name, as nodes, the one open last at the end.
    open: Vec<Vec<usize>>,
    /// The tags still to come.
    counts: Counts,
    /// The raw-text element, as its row, that is open, and whether it is
    /// on the stack (it is not past the depth cap).
    raw: Option<(usize, bool)>,
}

impl Tree {
    /// The tree of `pieces`, which are the parts of one container in order.
    pub fn new(pieces: Vec<Piece<'_>>) -> Self {
        let mut builder = Builder {
            nodes: Vec::new(),
            stack: Vec::new(),
            open: vec![Vec::new(); TABLE.len()],
            counts: Counts::of(&pieces),
            raw: None,
        };
        for (piece, content) in pieces.into_iter().enumerate() {
            match content {
                Piece::Html(tokens) => {
                    for (token, bytes) in tokens {
                        builder.token(token, Span { piece, bytes });
                    }
                }
                Piece::Source => builder.leaf(Span::none(piece), NodeKind::Source),
                Piece::Markdown => builder.markdown(piece),
            }
        }
        builder.finish()
    }
}

impl Span {
    fn none(piece: usize) -> Self {
        Self { piece, bytes: 0..0 }
    }
}

impl Builder {
    fn top(&self) -> Option<usize> {
        self.stack.last().copied()
    }

    fn leaf(&mut self, at: Span, kind: NodeKind) {
        let parent = self.top();
        self.nodes.push(Node { parent, at, kind });
    }

    fn token(&mut self, token: Token<'_>, at: Span) {
        if let Some((raw, pushed)) = self.raw {
            // Nothing is a tag until the end tag of the element itself.
            return match &token {
                Token::Close(name) if elements::index(name) == Some(raw) => {
                    self.raw = None;
                    self.counts.ends[raw] = self.counts.ends[raw].saturating_sub(1);
                    match pushed {
                        true => self.pop_to(self.open[raw].last().copied(), Closing::End(at)),
                        false => self.leaf(at, NodeKind::Unpaired { end: true }),
                    }
                }
                _ => self.leaf(at, NodeKind::Text),
            };
        }
        match token {
            Token::Text(_) => self.leaf(at, NodeKind::Text),
            Token::Comment => self.leaf(at, NodeKind::Comment),
            Token::Open(tag) => self.start(tag, at),
            Token::Close(name) => self.end(name, at),
        }
    }

    /// Rule 1.
    fn start(&mut self, tag: Tag, at: Span) {
        let index = elements::index(&tag.name);
        // The open elements that this start tag ends: a `<p>` by any block,
        // a void one such as `<hr>` too.
        if index.is_some_and(|index| starts_block(&TABLE[index])) {
            self.end_open_p();
        }
        let Some(index) = index.filter(|&i| nests(&TABLE[i])) else {
            return self.leaf(
                at,
                NodeKind::Tag {
                    name: tag.name,
                    end: false,
                },
            );
        };
        let row = &TABLE[index];
        self.counts.starts[index] = self.counts.starts[index].saturating_sub(1);
        let pushed = self.stack.len() < MAX_DEPTH;
        if row.kind == Kind::RawText {
            self.raw = Some((index, pushed));
        }
        if !pushed {
            return self.leaf(at, NodeKind::Unpaired { end: false });
        }
        let node = self.nodes.len();
        self.leaf(
            at,
            NodeKind::Element(Opened {
                row,
                tag,
                closing: Closing::Unpaired,
            }),
        );
        self.stack.push(node);
        self.open[index].push(node);
    }

    /// Rule 2.
    fn end(&mut self, name: String, at: Span) {
        let Some(index) = elements::index(&name).filter(|&i| nests(&TABLE[i])) else {
            return self.leaf(at, NodeKind::Tag { name, end: true });
        };
        self.counts.ends[index] = self.counts.ends[index].saturating_sub(1);
        let unpaired = NodeKind::Unpaired { end: true };
        let Some(&element) = self.open[index].last() else {
            return self.leaf(at, unpaired);
        };
        // An element above it with an end tag of its own to come: this end
        // tag is the one too many. The element open last of each name is the
        // one that matters, so this costs a look at each name, not at each
        // open element.
        let blocked = (0..TABLE.len()).any(|other| {
            let row = &TABLE[other];
            other != index
                && nests(row)
                && !row.end_optional
                && self.open[other].last().is_some_and(|&top| top > element)
                && self.counts.ends[other] >= self.counts.starts[other] + self.open[other].len()
        });
        if blocked {
            return self.leaf(at, unpaired);
        }
        self.pop_to(Some(element), Closing::End(at));
    }

    /// Rule 3.
    fn markdown(&mut self, piece: usize) {
        self.end_open_p();
        self.leaf(Span::none(piece), NodeKind::Markdown);
    }

    /// Ends the open elements that the start of a block ends: a `<p>`.
    fn end_open_p(&mut self) {
        while let Some(top) = self.top()
            && let NodeKind::Element(open) = &self.nodes[top].kind
            && open.row.end_optional
            && open.row.ended_by == EndedBy::AnyBlock
        {
            self.pop(Closing::Implied);
        }
    }

    /// Takes the element open last off the stack, ended as `closing` says.
    fn pop(&mut self, closing: Closing) {
        let node = self.stack.pop().expect("an element is open");
        let NodeKind::Element(open) = &mut self.nodes[node].kind else {
            unreachable!("the stack holds elements");
        };
        let index = elements::index(open.row.name).expect("a row of the table");
        open.closing = closing;
        self.open[index].pop();
    }

    /// Ends `element`, as `closing` says, and the elements open above it:
    /// one whose end tag may be left out is whole, and any other is an
    /// unpaired start tag.
    fn pop_to(&mut self, element: Option<usize>, closing: Closing) {
        let Some(element) = element else { return };
        while let Some(&top) = self.stack.last() {
            if top == element {
                return self.pop(closing);
            }
            let NodeKind::Element(open) = &self.nodes[top].kind else {
                unreachable!("the stack holds elements");
            };
            self.pop(given_up(open.row));
        }
    }

    /// Rule 4.
    fn finish(mut self) -> Tree {
        while !self.stack.is_empty() {
            let top = *self.stack.last().expect("an element is open");
            let NodeKind::Element(open) = &self.nodes[top].kind else {
                unreachable!("the stack holds elements");
            };
            self.pop(given_up(open.row));
        }
        Tree { nodes: self.nodes }
    }
}

#[cfg(test)]
mod tests;
