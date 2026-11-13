//! The bridge between places and the addresses that painting still uses.
//!
//! The search finds its matches as ranges of the document, and the painting
//! of them is still done the way it was before places: by a line and the
//! bytes of its text in the cell layout, and by a page and the bytes of one
//! of its texts in reading mode. This file turns the one into the other.
//!
//! It goes when painting moves to the engines, which paint a range of the
//! document themselves: the next task deletes this file, [`Mark`] and what
//! reads it.

use std::ops::Range;
use std::rc::Rc;

use super::{At, Map};
use crate::typeset::position::Pos;
use crate::typeset::reading::page::ItemRef;

/// Where some of a range is, as painting finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// The bytes of the text of a line of the cell layout, which is the text
    /// of its spans one after another.
    Bytes { line: usize, bytes: Range<usize> },
    /// The bytes of a text of a page of reading mode, which is named by the
    /// address of the page's parts.
    Glyphs {
        page: usize,
        item: ItemRef,
        range: Range<usize>,
    },
}

/// The marks of `range`: what shows it of each line of the cell layout it is
/// on and of each text of a page of reading mode. A range that a line break
/// splits gives a mark on each line; the space the break takes is no part of
/// either.
pub fn marks(map: &Map, range: &Range<Pos>) -> Vec<Mark> {
    let mut found: Vec<Mark> = Vec::new();
    if range.start >= range.end {
        return found;
    }
    let first = map.texts.partition_point(|shown| shown.hi <= range.start);
    for shown in &map.texts[first..] {
        if shown.lo >= range.end {
            break;
        }
        if shown.hi <= range.start {
            continue;
        }
        let bytes = Map::shown_bytes(shown, range);
        match &shown.at {
            At::Line { line, at } => add(
                &mut found,
                Mark::Bytes {
                    line: *line,
                    bytes: at + bytes.start..at + bytes.end,
                },
            ),
            At::Item { page, item } if !shown.origin.picture => {
                let page = Rc::as_ptr(&map.pages[*page].parts) as usize;
                let item = *item;
                add(
                    &mut found,
                    Mark::Glyphs {
                        page,
                        item,
                        range: bytes,
                    },
                );
            }
            At::Item { .. } | At::Row(_) | At::Drawn { .. } => {}
        }
    }
    found
}

/// Adds `mark`, joined to the last one when it goes on from it.
fn add(found: &mut Vec<Mark>, mark: Mark) {
    match (found.last_mut(), &mark) {
        (
            Some(Mark::Bytes { line, bytes }),
            Mark::Bytes {
                line: next,
                bytes: more,
            },
        ) if line == next && bytes.end == more.start => bytes.end = more.end,
        (
            Some(Mark::Glyphs { page, item, range }),
            Mark::Glyphs {
                page: next,
                item: same,
                range: more,
            },
        ) if *page == *next && *item == *same && range.end == more.start => range.end = more.end,
        _ => found.push(mark),
    }
}
