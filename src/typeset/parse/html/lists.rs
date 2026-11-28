//! The `<ul>` and `<ol>` of a container read as lists of the model, each `<li>` an item whose blocks are read as a `<details>` reads its own.
//!
//! The list reads its `<li>` children itself, and drops the white space and
//! comments between them. Every `<li>` it reads is an item, also an empty
//! one that something ended, as in `<li><li>b`. A list that holds anything
//! else has been dissolved before it gets here (`Holds::Only`), and an `<li>`
//! that no list reads is dissolved on its own, so a list with no item at all
//! is left to the caller, as source.
//!
//! - `<ol start>` numbers from a number of at most nine digits, the most
//!   that comrak takes, and from 1 for anything else; a `<ul>` has no start.
//! - The list is tight unless an item holds two blocks that are not lists, so
//!   an item with its nested lists is tight, as `- a` with `  - b` is.
//! - The places are those of a Markdown list: the list's own, and for each
//!   item no place of its own, only the places of its blocks.

use super::blocks::Reader;
use super::tree::{NodeKind, Opened};
use crate::typeset::model::{Block, List, ListItem};
use crate::typeset::parse::Position;

/// The most digits of a number that `start` may have: comrak's limit.
const START_DIGITS: usize = 9;

impl Reader<'_> {
    /// The list that the `<ul>` or `<ol>` at `node`, `open`, is, with its
    /// place `position`, onto `out`; `false` when it holds no `<li>` and is
    /// not a list.
    pub(super) fn list(
        &mut self,
        node: usize,
        open: &Opened,
        position: Position,
        out: &mut Vec<(Block, Position)>,
    ) -> bool {
        let tree = self.tree;
        let items: Vec<usize> = (self.children[node].iter().copied())
            .filter(|&child| matches!(tree.nodes[child].kind, NodeKind::Element(_)))
            .collect();
        if items.is_empty() {
            return false;
        }
        let mut read = Vec::with_capacity(items.len());
        let mut places = Vec::with_capacity(items.len());
        for item in items {
            // What an item holds is placed in its HTML blocks, as what a
            // `<details>` holds is.
            let blocks = self.inside(item, None, None);
            let (blocks, children): (Vec<_>, Vec<_>) = blocks.into_iter().unzip();
            read.push(ListItem {
                checked: None,
                blocks,
            });
            places.push(Position { at: None, children });
        }
        let tight = read.iter().all(|item| {
            let flat = (item.blocks.iter()).filter(|block| !matches!(block, Block::List(_)));
            flat.count() < 2
        });
        let list = Block::List(List {
            start: (open.row.name == "ol").then(|| start(open)),
            tight,
            footnotes: false,
            items: read,
        });
        out.push((
            list,
            Position {
                children: places,
                ..position
            },
        ));
        true
    }
}

/// The number that the `<ol>` `open` starts from: its `start` when that is a
/// number of at most `START_DIGITS` digits, else 1.
fn start(open: &Opened) -> u64 {
    open.tag
        .get("start")
        .map(str::trim_ascii)
        .filter(|digits| {
            (1..=START_DIGITS).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
        })
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(1)
}
