//! The tree of the HTML in a container read as the blocks of the model, by the table of elements and the level of HTML.
//!
//! The parts of a container (the document, a quote, a list item) come in
//! order: HTML blocks as comrak cut them, and Markdown blocks already read.
//! The tree of `tree.rs` is made of them once, and walked once, each node
//! read once:
//!
//! - A run of text leaves (text, comments and the tags of text elements) in
//!   one HTML block is one paragraph, joined by the inline join; a run of
//!   tags with nothing to show is left as written, and so are comments alone
//!   in their HTML block.
//! - A Markdown block is itself.
//! - A block element is what its row says: a `<p>`, `<div>` or `<center>`
//!   that centres is a centred block, one that does not is transparent, a
//!   `<details>` with its `<summary>` is a details, `<h1>` to `<h6>` a heading
//!   and `<pre>` a code block. An element that holds what its row does not
//!   allow shows its two tags as dim tags, and what it holds is read on its
//!   own. An element that is source for now is its source as written when
//!   no Markdown block is in it, and else its dim tags and what it holds.
//! - An unpaired tag is a dim tag, a block of its own that cuts a run of
//!   text, and what an unpaired element holds is read as what the element
//!   round it holds.
//!
//! A block read out of an HTML block has its place in it: the whole HTML
//! block when it is all there is of it, else its own line, inside the
//! element read round it or the HTML block.

use comrak::nodes::{LineColumn, Sourcepos};

use super::elements::{Holds, HtmlLevel, HtmlTags, Kind, Treatment, get, is_centred};
use super::inline::{Leaf, run_inlines};
use super::tree::{Closing, NodeKind, Opened, Piece, Span, Tree};
use super::{decode_entities, id_of, spanned_tokens};
use crate::typeset::model::{Block, Inline};
use crate::typeset::parse::Position;

/// A part of a container, as `parse` gives it.
pub enum Part {
    /// An HTML block as comrak cut it: its text, and where it is.
    Html { literal: String, at: Sourcepos },
    /// The blocks that a Markdown block is read as.
    Markdown(Vec<(Block, Position)>),
}

/// An HTML block of the container, with where its lines start.
struct HtmlBlock<'a> {
    literal: &'a str,
    at: Sourcepos,
    /// The byte after each line break of `literal`.
    breaks: Vec<usize>,
}

impl HtmlBlock<'_> {
    /// The line and column in the document of the byte `byte`, which may be
    /// any byte of a character: the bytes are counted.
    fn at(&self, byte: usize) -> LineColumn {
        let line = self.breaks.partition_point(|&start| start <= byte);
        let start = self.at.start;
        match line {
            0 => LineColumn {
                line: start.line,
                column: start.column + byte,
            },
            _ => LineColumn {
                line: start.line + line,
                column: start.column.saturating_sub(1) + byte - (self.breaks[line - 1] - 1),
            },
        }
    }

    /// Whether `bytes` are all there is of the block but white space.
    fn alone(&self, bytes: &std::ops::Range<usize>) -> bool {
        let blank = |text: &str| text.trim_ascii().is_empty();
        blank(&self.literal[..bytes.start]) && blank(&self.literal[bytes.end..])
    }
}

/// The blocks of a container of `parts`, in order, with their places.
pub(in crate::typeset::parse) fn read(
    parts: Vec<Part>,
    level: HtmlLevel,
    tags: HtmlTags,
) -> Vec<(Block, Position)> {
    let mut texts = Vec::with_capacity(parts.len());
    let mut markdown = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            Part::Html { literal, at } => {
                texts.push(Some((literal, at)));
                markdown.push(None);
            }
            Part::Markdown(blocks) => {
                texts.push(None);
                markdown.push(Some(blocks));
            }
        }
    }
    let html: Vec<Option<HtmlBlock<'_>>> = (texts.iter())
        .map(|text| {
            text.as_ref().map(|(literal, at)| HtmlBlock {
                literal,
                at: *at,
                breaks: (literal.match_indices('\n'))
                    .map(|(at, _)| at + 1)
                    .collect(),
            })
        })
        .collect();
    let pieces = (html.iter())
        .map(|block| match block {
            Some(block) => spanned_tokens(block.literal).map_or(Piece::Source, Piece::Html),
            None => Piece::Markdown,
        })
        .collect();
    let tree = Tree::new(pieces);
    let count = tree.nodes.len();
    // The nodes in each node, and in the container, which is last; the last
    // node inside each; and whether a block of Markdown is inside it.
    let mut children = vec![Vec::new(); count + 1];
    let mut last: Vec<usize> = (0..count).collect();
    let mut markdown_in = vec![false; count];
    // The last byte that each node and what is in it take, as an HTML block
    // and a byte in it: an end tag is no node, so it is the end of its
    // element, and a block that cannot be read is all of its literal.
    let mut extent: Vec<(usize, usize)> = tree
        .nodes
        .iter()
        .map(|node| {
            let own = match &node.kind {
                NodeKind::Element(open) => match &open.closing {
                    Closing::End(end) => end,
                    _ => &node.at,
                },
                _ => &node.at,
            };
            match (&node.kind, &html[node.at.piece]) {
                (NodeKind::Source, Some(block)) => (node.at.piece, block.literal.len()),
                _ => (own.piece, own.bytes.end),
            }
        })
        .collect();
    for (at, node) in tree.nodes.iter().enumerate() {
        children[node.parent.unwrap_or(count)].push(at);
    }
    for (at, node) in tree.nodes.iter().enumerate().rev() {
        if let Some(parent) = node.parent {
            extent[parent] = extent[parent].max(extent[at]);
            last[parent] = last[parent].max(last[at]);
            markdown_in[parent] |= markdown_in[at] || matches!(node.kind, NodeKind::Markdown);
        }
    }
    let mut reader = Reader {
        level,
        tags,
        tree: &tree,
        html: &html,
        markdown,
        children,
        last,
        extent,
        markdown_in,
    };
    let mut out = Vec::new();
    let mut run = Vec::new();
    reader.read_children(count, None, &mut out, &mut run);
    reader.flush(&mut run, None, &mut out);
    give_anchors(&mut out);
    out
}

/// The element read round a block, when it is in one HTML block: blocks of
/// that HTML block inside it are placed inside it.
type Round = Option<(usize, Sourcepos)>;

struct Reader<'t> {
    /// The level of HTML that is read.
    level: HtmlLevel,
    /// What becomes of the tags that have no effect.
    tags: HtmlTags,
    tree: &'t Tree,
    html: &'t [Option<HtmlBlock<'t>>],
    /// The blocks of each block of Markdown, taken when it is read.
    markdown: Vec<Option<Vec<(Block, Position)>>>,
    children: Vec<Vec<usize>>,
    last: Vec<usize>,
    /// The last byte in each node, as `read` makes it.
    extent: Vec<(usize, usize)>,
    markdown_in: Vec<bool>,
}

impl<'t> Reader<'t> {
    fn block(&self, piece: usize) -> &'t HtmlBlock<'t> {
        self.html[piece].as_ref().expect("an HTML block")
    }

    fn written(&self, at: &Span) -> &'t str {
        &self.block(at.piece).literal[at.bytes.clone()]
    }

    /// Where a block read from `bytes` of the HTML block `piece` is: the
    /// whole HTML block when it is all of it, else inside the element `round`
    /// or the HTML block, on the line of its first byte.
    fn place(&self, piece: usize, bytes: &std::ops::Range<usize>, round: Round) -> Position {
        let block = self.block(piece);
        if block.alone(bytes) {
            return Position::exact(block.at);
        }
        let line = block.at(bytes.start).line;
        match round {
            Some((inside, at)) if inside == piece => Position::inside(at, line),
            _ => Position::inside(block.at, line),
        }
    }

    /// The bytes `bytes` of the HTML block `piece` as a block of source: the
    /// whole HTML block when they are all of it.
    fn source(
        &self,
        piece: usize,
        mut bytes: std::ops::Range<usize>,
        round: Round,
    ) -> (Block, Position) {
        let block = self.block(piece);
        let position = self.place(piece, &bytes, round);
        // The last of its HTML block keeps the line break that ends it.
        if block.literal[bytes.end..].trim_ascii().is_empty() {
            bytes.end = block.literal.len();
        }
        let source = match block.alone(&bytes) {
            true => block.literal.to_owned(),
            false => block.literal[bytes].to_owned(),
        };
        (Block::Html(source), position)
    }

    /// A tag that does nothing, as a block of its own, or nothing when the
    /// tags with no effect are hidden.
    fn dim(&self, at: &Span, round: Round, out: &mut Vec<(Block, Position)>) {
        if self.tags == HtmlTags::Dim {
            out.push(self.source(at.piece, at.bytes.clone(), round));
        }
    }

    /// Reads the nodes in `parent` onto `out`, a run of text going on in
    /// `run`.
    fn read_children(
        &mut self,
        parent: usize,
        round: Round,
        out: &mut Vec<(Block, Position)>,
        run: &mut Vec<usize>,
    ) {
        self.read_children_but(parent, None, round, out, run);
    }

    /// Reads the nodes in `parent` but `skip` onto `out`, a run of text going
    /// on in `run`.
    fn read_children_but(
        &mut self,
        parent: usize,
        skip: Option<usize>,
        round: Round,
        out: &mut Vec<(Block, Position)>,
        run: &mut Vec<usize>,
    ) {
        for at in 0..self.children[parent].len() {
            let child = self.children[parent][at];
            if Some(child) == skip {
                // What is read apart from the run, such as the summary of a
                // details, cuts it: its bytes are not the run's.
                self.flush(run, round, out);
                continue;
            }
            let node = &self.tree.nodes[child];
            match &node.kind {
                NodeKind::Text | NodeKind::RawTag { .. } | NodeKind::Comment => {
                    self.extend(run, child, round, out);
                }
                NodeKind::Tag { tag, end } => match get(&tag.name) {
                    // A void block, such as `<hr>`, is source for now.
                    Some(row) if row.kind == Kind::VoidBlock && !end => {
                        self.flush(run, round, out);
                        out.push(self.source(node.at.piece, node.at.bytes.clone(), round));
                    }
                    _ => self.extend(run, child, round, out),
                },
                NodeKind::Markdown => {
                    self.flush(run, round, out);
                    out.extend(self.markdown[node.at.piece].take().unwrap_or_default());
                }
                NodeKind::Source => {
                    self.flush(run, round, out);
                    let block = self.block(node.at.piece);
                    out.push((
                        Block::Html(block.literal.to_owned()),
                        Position::exact(block.at),
                    ));
                }
                NodeKind::Unpaired => {
                    self.flush(run, round, out);
                    self.dim(&node.at, round, out);
                }
                NodeKind::Element(open) => {
                    // A raw-text element that is no block, such as a
                    // `<script>`, is a tag of its run, as written.
                    if open.row.kind == Kind::RawText
                        && open.row.treatment == Treatment::Source
                        && self.whole_in_one(child, open).is_some()
                    {
                        self.extend(run, child, round, out);
                        continue;
                    }
                    self.flush(run, round, out);
                    if open.closing == Closing::Unpaired {
                        // Its tag does nothing, and what it holds is what the
                        // element round it holds.
                        self.dim(&node.at, round, out);
                        self.read_children(child, round, out, run);
                        continue;
                    }
                    self.element(child, open, round, out);
                }
            }
        }
    }

    /// Adds the leaf `node` to `run`. A run ends with its HTML block: what a
    /// blank line sets apart is two paragraphs.
    fn extend(
        &self,
        run: &mut Vec<usize>,
        node: usize,
        round: Round,
        out: &mut Vec<(Block, Position)>,
    ) {
        let piece = self.tree.nodes[node].at.piece;
        if run
            .last()
            .is_some_and(|&last| self.tree.nodes[last].at.piece != piece)
        {
            self.flush(run, round, out);
        }
        run.push(node);
    }

    /// The bytes of `node`, the element `open`, from its start tag to the end
    /// of its end tag, when both are in one HTML block.
    fn whole_in_one(&self, node: usize, open: &Opened) -> Option<std::ops::Range<usize>> {
        let start = &self.tree.nodes[node].at;
        match &open.closing {
            Closing::End(end) if end.piece == start.piece => Some(start.bytes.start..end.bytes.end),
            _ => None,
        }
    }

    /// The span of `node`, the element `open`, from its start tag to its end
    /// tag, or else to the end of the last node in it: the HTML block it
    /// starts in and its byte there, and the same for where it ends.
    fn ends(&self, node: usize, open: &Opened) -> ((usize, usize), (usize, usize)) {
        let start = &self.tree.nodes[node].at;
        let end = match &open.closing {
            Closing::End(end) => end.clone(),
            _ => {
                let last = &self.tree.nodes[self.last[node]];
                let (piece, byte) = match last.kind {
                    NodeKind::Markdown => (start.piece, start.bytes.end),
                    _ => self.extent[node],
                };
                return ((start.piece, start.bytes.start), (piece, byte));
            }
        };
        ((start.piece, start.bytes.start), (end.piece, end.bytes.end))
    }

    /// Where the element `open` at `node` is, and what it is as the element
    /// round the blocks in it.
    fn element_place(&self, node: usize, open: &Opened) -> (Position, Round) {
        let ((first, start), (last, end)) = self.ends(node, open);
        let start_byte = start;
        let (from, to) = (self.block(first), self.block(last));
        if first == last && from.alone(&(start..end)) {
            return (Position::exact(from.at), Some((first, from.at)));
        }
        // Tags that start or end their HTML block take its edge.
        let start = match from.literal[..start].trim_ascii().is_empty() {
            true => from.at.start,
            false => from.at(start),
        };
        let end = match (to.literal[end..].trim_ascii().is_empty(), first == last) {
            (true, _) => to.at.end,
            (false, true) => to.at(end.saturating_sub(1).max(start_byte)),
            (false, false) => to.at(end.saturating_sub(1)),
        };
        let at = Sourcepos { start, end };
        let round = (first == last).then_some((first, at));
        (Position::exact(at), round)
    }

    /// The two tags of `open` at `node` as dim tags, and what it holds read
    /// on its own between them.
    fn dissolve(
        &mut self,
        node: usize,
        open: &Opened,
        round: Round,
        out: &mut Vec<(Block, Position)>,
    ) {
        self.dim(&self.tree.nodes[node].at, round, out);
        let mut run = Vec::new();
        self.read_children(node, round, out, &mut run);
        self.flush(&mut run, round, out);
        if let Closing::End(end) = &open.closing {
            self.dim(end, round, out);
        }
    }

    /// Whether `node` holds nothing but white space.
    fn holds_nothing(&self, node: usize) -> bool {
        self.children[node].iter().all(|&child| self.blank(child))
    }

    /// Whether `node` is white space in a run of text.
    fn blank(&self, node: usize) -> bool {
        matches!(self.tree.nodes[node].kind, NodeKind::Text)
            && self
                .written(&self.tree.nodes[node].at)
                .trim_ascii()
                .is_empty()
    }

    /// Whether `node` holds no more than its row allows: an element that holds
    /// text holds no block.
    fn holds_allowed(&self, node: usize, open: &Opened) -> bool {
        open.row.holds != Holds::Text
            || self.children[node]
                .iter()
                .all(|&child| match &self.tree.nodes[child].kind {
                    NodeKind::Text
                    | NodeKind::RawTag { .. }
                    | NodeKind::Comment
                    | NodeKind::Tag { .. } => true,
                    NodeKind::Element(inner) if self.is_summary_heading(open, inner, child) => true,
                    NodeKind::Element(inner) => {
                        inner.row.kind == Kind::RawText
                            && inner.row.treatment == Treatment::Source
                            && self.whole_in_one(child, inner).is_some()
                    }
                    _ => false,
                })
    }

    /// Whether `inner` at `node` is a whole heading in the summary `open`,
    /// as GitHub allows: it holds text, and is ended by its own end tag.
    fn is_summary_heading(&self, open: &Opened, inner: &Opened, node: usize) -> bool {
        open.row.treatment == Treatment::Summary
            && matches!(inner.row.treatment, Treatment::Heading(_))
            && matches!(inner.closing, Closing::End(_))
            && self.holds_allowed(node, inner)
    }

    /// The blocks of the element `open`, ended, at `node`.
    fn element(
        &mut self,
        node: usize,
        open: &Opened,
        round: Round,
        out: &mut Vec<(Block, Position)>,
    ) {
        // An element that the level does not read has no effect: its tags
        // are dim and what it holds is read on its own.
        if open.row.is_beyond(self.level) || !self.holds_allowed(node, open) {
            return self.dissolve(node, open, round, out);
        }
        // An element that something else ended before it held anything, such
        // as a `<p>` that a block of Markdown ends at once, shows its tag.
        if open.closing == Closing::Implied && self.holds_nothing(node) {
            return self.dim(&self.tree.nodes[node].at, round, out);
        }
        let (position, inner_round) = self.element_place(node, open);
        match open.row.treatment {
            Treatment::Wrapper(_) if open.row.centres_with(&open.tag) => {
                let blocks = self.inside(node, None, inner_round);
                if blocks.is_empty() {
                    return;
                }
                out.push(wrap(Block::Centered, blocks, position));
            }
            // An element with nothing to draw is as if its tags were not
            // there.
            Treatment::Wrapper(_) => {
                let mut run = Vec::new();
                self.read_children(node, round, out, &mut run);
                self.flush(&mut run, round, out);
            }
            Treatment::Details => {
                // What a `<details>` holds is placed in its HTML blocks, not
                // in the details.
                let summary = self.summary(node);
                let blocks = self.inside(node, summary, None);
                let summary = match summary {
                    Some(summary) => self.inlines(summary),
                    None => vec![Inline::Text("Details".into())],
                };
                out.push(wrap(
                    |blocks| Block::Details { summary, blocks },
                    blocks,
                    position,
                ));
            }
            Treatment::Heading(level) => {
                let content = self.inlines(node);
                if content.is_empty() {
                    return;
                }
                let heading = Block::Heading {
                    level,
                    content,
                    id: id_of(&open.tag),
                };
                let start = &self.tree.nodes[node].at;
                if is_centred(&open.tag) {
                    let line = self.block(start.piece).at(start.bytes.start).line;
                    let at = position.sourcepos();
                    let inner = vec![(heading, Position::inside(at, line))];
                    out.push(wrap(Block::Centered, inner, position));
                } else {
                    // A heading is placed as a paragraph is, not as an
                    // element that holds blocks.
                    let ((first, from), (last, to)) = self.ends(node, open);
                    let to = if first == last { to } else { from };
                    out.push((heading, self.place(first, &(from..to), round)));
                }
            }
            Treatment::Pre => match self.code(node, open) {
                Some(code) => {
                    let ((first, from), (_, to)) = self.ends(node, open);
                    out.push((code, self.place(first, &(from..to), round)));
                }
                None => self.as_written(node, open, round, out),
            },
            // A `<summary>` that is not the summary of a `<details>` has no
            // effect: its tags are dim and its text is read on its own.
            Treatment::Summary => self.dissolve(node, open, round, out),
            // The elements that are source for now.
            _ => self.as_written(node, open, round, out),
        }
    }

    /// An element left as source for now: as written when no block of
    /// Markdown is in it, a block for its part of each HTML block it spans
    /// when blank lines split it; else its tags dim and what it holds on its
    /// own.
    fn as_written(
        &mut self,
        node: usize,
        open: &Opened,
        round: Round,
        out: &mut Vec<(Block, Position)>,
    ) {
        if self.markdown_in[node] {
            return self.dissolve(node, open, round, out);
        }
        // With no Markdown block in it, every piece it spans is HTML.
        let ((first, start), (last, end)) = self.ends(node, open);
        for piece in first..=last {
            let from = if piece == first { start } else { 0 };
            let to = match piece == last {
                true => end,
                false => self.block(piece).literal.len(),
            };
            out.push(self.source(piece, from..to, round));
        }
    }

    /// The blocks that `node` holds, but for the node `skip`.
    fn inside(&mut self, node: usize, skip: Option<usize>, round: Round) -> Vec<(Block, Position)> {
        let mut blocks = Vec::new();
        let mut run = Vec::new();
        self.read_children_but(node, skip, round, &mut blocks, &mut run);
        self.flush(&mut run, round, &mut blocks);
        give_anchors(&mut blocks);
        blocks
    }

    /// The `<summary>` of the `<details>` at `node`: the first that is whole
    /// and holds only text.
    fn summary(&self, node: usize) -> Option<usize> {
        self.children[node].iter().copied().find(|&child| {
            matches!(&self.tree.nodes[child].kind,
                NodeKind::Element(open) if open.row.treatment == Treatment::Summary
                    && open.closing != Closing::Unpaired
                    && self.holds_allowed(child, open))
        })
    }

    /// The inlines of the text that `node` holds. A heading in a summary
    /// gives what it holds, in order with the text beside it, and a space
    /// apart from it, as a browser draws it in a box of its own.
    fn inlines(&self, node: usize) -> Vec<Inline> {
        let in_summary = matches!(&self.tree.nodes[node].kind,
            NodeKind::Element(open) if open.row.treatment == Treatment::Summary);
        let leaves = self.children[node].iter().flat_map(|&child| {
            let heading = in_summary
                && matches!(&self.tree.nodes[child].kind,
                    NodeKind::Element(inner) if matches!(inner.row.treatment, Treatment::Heading(_)));
            if !heading {
                return self.leaf(child).into_iter().collect::<Vec<_>>();
            }
            // The spaces at the ends of the summary and doubled ones are
            // dropped when the leaves are joined.
            let space = Leaf::Text(" ");
            let held = self.children[child].iter().filter_map(|&inner| self.leaf(inner));
            std::iter::once(space)
                .chain(held)
                .chain(std::iter::once(Leaf::Text(" ")))
                .collect()
        });
        run_inlines(leaves, self.level, self.tags)
    }

    /// The leaf of a run that `node` is.
    fn leaf(&self, node: usize) -> Option<Leaf<'t>> {
        let at = &self.tree.nodes[node].at;
        Some(match &self.tree.nodes[node].kind {
            NodeKind::Text | NodeKind::RawTag { .. } => Leaf::Text(self.written(at)),
            NodeKind::Comment => Leaf::Comment,
            NodeKind::Tag { tag, end } => Leaf::Tag(self.written(at), tag, *end),
            NodeKind::Element(open) => {
                let bytes = self.whole_in_one(node, open)?;
                Leaf::Source(&self.block(at.piece).literal[bytes])
            }
            _ => return None,
        })
    }

    /// The run `run` of text leaves as the block it makes, onto `out`: a
    /// paragraph, or, when it shows nothing but tags and comments, its source
    /// as written. A run of white space makes nothing.
    fn flush(&self, run: &mut Vec<usize>, round: Round, out: &mut Vec<(Block, Position)>) {
        let leaves = std::mem::take(run);
        let Some(first) = leaves.iter().position(|&node| !self.blank(node)) else {
            return;
        };
        let last = (leaves.iter())
            .rposition(|&node| !self.blank(node))
            .expect("one is not blank");
        let leaves = &leaves[first..=last];
        // A run lies in one HTML block: `extend` ends it at the next.
        let piece = self.tree.nodes[leaves[0]].at.piece;
        debug_assert!(
            (leaves.iter()).all(|&node| self.tree.nodes[node].at.piece == piece),
            "a run in more than one HTML block"
        );
        let start = self.tree.nodes[leaves[0]].at.bytes.start;
        let end = self.end_of(leaves[leaves.len() - 1]);
        let inlines = run_inlines(
            leaves.iter().filter_map(|&node| self.leaf(node)),
            self.level,
            self.tags,
        );
        let shows = inlines.iter().any(|inline| match inline {
            Inline::Html(_) => false,
            Inline::Text(text) => !text.trim_ascii().is_empty(),
            _ => true,
        });
        let tags = (leaves.iter()).any(|&node| {
            !matches!(
                self.tree.nodes[node].kind,
                NodeKind::Comment | NodeKind::Text
            )
        });
        // Nothing but tags that have no effect, and the comments and white
        // space between them: they can be left out.
        let dim = (leaves.iter()).all(|&node| {
            matches!(
                self.tree.nodes[node].kind,
                NodeKind::Comment | NodeKind::Text | NodeKind::Tag { .. }
            )
        });
        if !shows {
            if dim && tags && self.tags == HtmlTags::Hide {
                return;
            }
            // Comments beside other things in their HTML block show nothing,
            // as in a browser; alone in it they are left as written, and so
            // are tags with nothing to show.
            if tags || self.block(piece).alone(&(start..end)) {
                out.push(self.source(piece, start..end, round));
            }
            return;
        }
        // The paragraph starts at its first word, not at the blank before it.
        let written = self.written(&self.tree.nodes[leaves[0]].at);
        let start = start + (written.len() - written.trim_ascii_start().len());
        let position = self.place(piece, &self.widened(leaves, start..end), round);
        out.push((Block::Paragraph(inlines), position));
    }

    /// `bytes`, those of the run `leaves`, widened over the tags of the
    /// elements round them that draw nothing of their own and hold nothing
    /// else, in the same HTML block: a paragraph that is all its HTML block
    /// shows, such as an image in a plain `<p>`, is placed as that block.
    fn widened(
        &self,
        leaves: &[usize],
        mut bytes: std::ops::Range<usize>,
    ) -> std::ops::Range<usize> {
        let piece = self.tree.nodes[leaves[0]].at.piece;
        let solid = |parent: usize, inner: &[usize]| {
            let mut own =
                (self.children[parent].iter().copied()).filter(|&child| !self.blank(child));
            inner.iter().all(|&node| own.next() == Some(node)) && own.next().is_none()
        };
        let mut inner: Vec<usize> = (leaves.iter().copied())
            .filter(|&node| !self.blank(node))
            .collect();
        let mut parent = self.tree.nodes[leaves[0]].parent;
        while let Some(element) = parent
            && let NodeKind::Element(open) = &self.tree.nodes[element].kind
            && matches!(open.row.treatment, Treatment::Wrapper(_))
            && !open.row.centres_with(&open.tag)
            && open.closing != Closing::Unpaired
            && solid(element, &inner)
        {
            let ((first, start), (last, end)) = self.ends(element, open);
            if first != piece || last != piece {
                break;
            }
            bytes = start.min(bytes.start)..end.max(bytes.end);
            inner = vec![element];
            parent = self.tree.nodes[element].parent;
        }
        bytes
    }

    /// The end of the bytes of the leaf `node`: of the whole element when it
    /// is one, such as a `<script>`.
    fn end_of(&self, node: usize) -> usize {
        match &self.tree.nodes[node].kind {
            NodeKind::Element(open) => self
                .whole_in_one(node, open)
                .map_or(self.tree.nodes[node].at.bytes.end, |bytes| bytes.end),
            _ => self.tree.nodes[node].at.bytes.end,
        }
    }

    /// The code block that the `<pre>` at `node` is, `open`: with one
    /// `<code>` in it or not, and only text between the tags, what a fence
    /// with that text would give. The language is the `lang` of the `<pre>`,
    /// else a `language-` or `lang-` class of the `<code>`. `None` for
    /// anything else, and for no code.
    fn code(&self, node: usize, open: &Opened) -> Option<Block> {
        let Closing::End(close) = &open.closing else {
            return None;
        };
        let start = &self.tree.nodes[node].at;
        let literal = self.block(start.piece).literal;
        let inside: Vec<&super::tree::Node> = (self.children[node].iter())
            .map(|&child| &self.tree.nodes[child])
            .collect();
        let blank = |node: &super::tree::Node| {
            matches!(node.kind, NodeKind::Text) && literal[node.at.bytes.clone()].trim().is_empty()
        };
        let code_tag = |node: &super::tree::Node, end: bool| matches!(&node.kind, NodeKind::RawTag { tag, end: e } if tag.name == "code" && *e == end);
        // The `<code>` right after the `<pre>`, past white space only. That
        // white space is part of the code, as in a browser, but for its first
        // newline.
        let (lead, from_code) = match inside.as_slice() {
            [first, after @ ..]
                if blank(first) && after.first().is_some_and(|n| code_tag(n, false)) =>
            {
                (&literal[first.at.bytes.clone()], after)
            }
            all => ("", all),
        };
        let (code, text_start, body) = match from_code {
            [tag, after @ ..] if code_tag(tag, false) => {
                let NodeKind::RawTag { tag: code, .. } = &tag.kind else {
                    unreachable!("a code tag")
                };
                (Some(code), tag.at.bytes.end, after)
            }
            all => (None, start.bytes.end, all),
        };
        let text_end = match (code.is_some(), body) {
            (false, []) => close.bytes.start,
            (false, [text]) if matches!(text.kind, NodeKind::Text) => close.bytes.start,
            (true, [end, rest @ ..])
                if code_tag(end, true) && rest.iter().all(|node| blank(node)) =>
            {
                end.at.bytes.start
            }
            (true, [text, end, rest @ ..])
                if matches!(text.kind, NodeKind::Text)
                    && code_tag(end, true)
                    && rest.iter().all(|node| blank(node)) =>
            {
                end.at.bytes.start
            }
            _ => return None,
        };
        if close.piece != start.piece {
            return None;
        }
        let lead = match code {
            Some(_) => drop_first_newline(lead),
            None => "",
        };
        // A browser drops the one newline right after the opening tag; here
        // that is the last one, whether the blanks before `<code>` stay or not.
        let raw = drop_first_newline(&literal[text_start..text_end]);
        if lead.trim().is_empty() && raw.trim().is_empty() {
            return None;
        }
        let mut text = lead.to_owned();
        text.push_str(&decode_entities(raw));
        // A fence keeps the newline that ends its last line.
        if !text.ends_with('\n') {
            text.push('\n');
        }
        let language = (open.tag.get("lang"))
            .and_then(|lang| lang.split_ascii_whitespace().next())
            .or_else(|| {
                code?
                    .get("class")?
                    .split_ascii_whitespace()
                    .find_map(|class| {
                        class
                            .strip_prefix("language-")
                            .or_else(|| class.strip_prefix("lang-"))
                            .filter(|lang| !lang.is_empty())
                    })
            });
        Some(Block::CodeBlock {
            language: language.map(str::to_owned),
            code: text.into(),
            whole_file: false,
        })
    }
}

/// `text` without the one newline that may start it.
fn drop_first_newline(text: &str) -> &str {
    text.strip_prefix("\r\n")
        .or_else(|| text.strip_prefix('\n'))
        .unwrap_or(text)
}

/// The anchors that `block` is, when it is a paragraph of nothing but the
/// places that `<a name>` and `<a id>` name and white space: it draws
/// nothing, so it is not a block of its own.
fn only_anchors(block: &Block) -> Option<Vec<Inline>> {
    let Block::Paragraph(content) = block else {
        return None;
    };
    let blank = |inline: &Inline| match inline {
        Inline::Anchor(_) | Inline::SoftBreak | Inline::LineBreak => true,
        Inline::Text(text) => text.trim_ascii().is_empty(),
        _ => false,
    };
    let anchors: Vec<Inline> = (content.iter())
        .filter(|inline| matches!(inline, Inline::Anchor(_)))
        .cloned()
        .collect();
    (!anchors.is_empty() && content.iter().all(blank)).then_some(anchors)
}

/// The inlines at the start of `block`, where a place can be given to it.
fn first_inlines(block: &mut Block) -> Option<&mut Vec<Inline>> {
    match block {
        Block::Paragraph(content) | Block::Heading { content, .. } => Some(content),
        Block::Centered(inner) => inner.first_mut().and_then(first_inlines),
        _ => None,
    }
}

/// The inlines at the end of `block`.
fn last_inlines(block: &mut Block) -> Option<&mut Vec<Inline>> {
    match block {
        Block::Paragraph(content) | Block::Heading { content, .. } => Some(content),
        Block::Centered(inner) => inner.last_mut().and_then(last_inlines),
        _ => None,
    }
}

/// Gives the places of the blocks that are nothing but anchors to the block
/// after them, which they stay in front of as jump targets, or else to the
/// block before them. A paragraph of nothing but anchors would draw an empty
/// paragraph, so it is not kept; but when there is no paragraph or heading in
/// the container to give them to, it is, as it was read, so that no jump
/// target is lost.
fn give_anchors(blocks: &mut Vec<(Block, Position)>) {
    if !blocks
        .iter()
        .any(|(block, _)| only_anchors(block).is_some())
    {
        return;
    }
    let mut out: Vec<(Block, Position)> = Vec::with_capacity(blocks.len());
    let mut carried: Vec<(Block, Position)> = Vec::new();
    for (mut block, position) in std::mem::take(blocks) {
        if only_anchors(&block).is_some() {
            carried.push((block, position));
            continue;
        }
        if !carried.is_empty() {
            match first_inlines(&mut block) {
                Some(content) => {
                    let before = anchors_of(&carried);
                    carried.clear();
                    content.splice(0..0, before);
                }
                // No block to give them to after them: the one before.
                None => give_back(&mut out, &mut carried),
            }
        }
        out.push((block, position));
    }
    give_back(&mut out, &mut carried);
    *blocks = out;
}

/// The anchors of the blocks `carried`, which are all only anchors.
fn anchors_of(carried: &[(Block, Position)]) -> Vec<Inline> {
    (carried.iter())
        .flat_map(|(block, _)| only_anchors(block).unwrap_or_default())
        .collect()
}

/// Gives `carried` to the end of the last block of `out`, if it has one that
/// can take them; they stay blocks of their own otherwise, after it.
fn give_back(out: &mut Vec<(Block, Position)>, carried: &mut Vec<(Block, Position)>) {
    if carried.is_empty() {
        return;
    }
    let anchors = anchors_of(carried);
    match out.last_mut().and_then(|(block, _)| last_inlines(block)) {
        Some(content) => {
            content.extend(anchors);
            carried.clear();
        }
        None => out.append(carried),
    }
}

/// The block that `make` makes of `blocks`, at `position`, with the places
/// of the blocks as its children.
fn wrap(
    make: impl FnOnce(Vec<Block>) -> Block,
    blocks: Vec<(Block, Position)>,
    position: Position,
) -> (Block, Position) {
    let (blocks, children): (Vec<_>, Vec<_>) = blocks.into_iter().unzip();
    (
        make(blocks),
        Position {
            children,
            ..position
        },
    )
}
