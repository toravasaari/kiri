//! The contents of the shown documents: their headings as a flat list, and
//! the heading the reader is in.
//!
//! The list is made from the named places of [`Targets`], not from the
//! documents again: a heading there has its id and its place, and this adds
//! its level and its text, read from the block at that place, or for a
//! heading written in a block of HTML from `Named::inner`, which
//! `targets()` has read. It does not know where it is drawn, nor how a
//! heading is shown.

use crate::typeset::anchors::anchor_text;
use crate::typeset::locate::Map;
use crate::typeset::model::{Block, Document};
use crate::typeset::position::Pos;
use crate::typeset::targets::{NamedKind, Targets};

/// One line of the contents: a heading, or a file when several are shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub file: usize,
    /// The block of the heading; empty for a file.
    pub path: Vec<usize>,
    /// Where the heading is: the block, or for a heading written in a
    /// block of HTML, the bytes of the element, from its start tag to the
    /// end of its end tag; the start of the file for a file.
    pub start: Pos,
    /// 1 to 6 for a heading, 0 for a file.
    pub level: u8,
    /// What the heading says, on one line; the name of a file.
    pub text: String,
    /// The id of the heading, as a link to `#id` names it, which is empty
    /// for a heading without text; `None` for a file. So `(file, id)` is
    /// a key of its own for each entry.
    pub id: Option<String>,
}

/// The entries of the shown documents, in the order of the documents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contents {
    entries: Vec<Entry>,
}

/// The contents of `files`, each a document and the name its header shows
/// when several files are shown, with `targets` of the same documents.
pub fn contents<'a>(
    files: impl IntoIterator<Item = (Option<&'a str>, &'a Document)>,
    targets: &Targets,
) -> Contents {
    let mut entries = Vec::new();
    // `named` is in the order of the documents, so a file's headings are
    // one run of it.
    let mut named = targets.named.iter().peekable();
    for (file, (name, document)) in files.into_iter().enumerate() {
        if let Some(name) = name {
            entries.push(Entry {
                file,
                path: Vec::new(),
                start: Pos::block(file, &[]),
                level: 0,
                text: one_line(name),
                id: None,
            });
        }
        let headings: Vec<_> =
            std::iter::from_fn(|| named.next_if(|named| named.range.start.file == file))
                .filter(|named| named.kind == NamedKind::Heading)
                .collect();
        if headings.is_empty() {
            continue;
        }
        // The blocks are listed once for the file, and only when one of its
        // headings is a block of its own: a file whose headings are all in
        // blocks of HTML costs nothing here.
        let blocks =
            (headings.iter().any(|named| named.inner.is_none())).then(|| document.blocks());
        entries.extend(headings.into_iter().filter_map(|named| {
            let path = &named.range.start.path;
            // A heading written in a block of HTML has its level and text
            // in `inner`, read by `targets()`: the block at its place holds
            // more than the heading.
            let (level, text) = match (&named.inner, &blocks) {
                (Some(inner), _) => (inner.level, one_line(&inner.text)),
                (None, Some(blocks)) => {
                    let at = blocks.binary_search_by(|(other, _)| other.cmp(path)).ok()?;
                    let Block::Heading { level, content } = blocks[at].1 else {
                        return None;
                    };
                    (*level, one_line(&anchor_text(content)))
                }
                (None, None) => return None,
            };
            Some(Entry {
                file,
                path: path.clone(),
                start: named.range.start.clone(),
                level,
                text,
                id: Some(named.id.clone()),
            })
        }));
    }
    Contents { entries }
}

/// `text` on one line: the runs of white space are one space, and any
/// other control character is U+FFFD, as `Span::new` has it, so that a
/// drawer that writes the text as it is sends no escape to the terminal.
fn one_line(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| {
            let safe = |c: char| if c.is_control() { '\u{fffd}' } else { c };
            word.chars().map(safe).collect()
        })
        .collect();
    words.join(" ")
}

impl Contents {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry the reader is in, with the first line of the view at
    /// `top`: the last one that the lines of `map` show at or above it, as an index of
    /// [`Contents::entries`]. `None` before the first one, and when there
    /// are no entries.
    ///
    /// The entries are in the order of the document, and so are the lines
    /// that show them, in either layout, so it is a binary search: for
    /// each probe, the line of an entry is asked of `map`. An entry that
    /// no line shows yet, as in a layout that is not done, is after `top`.
    /// A file entry is at the first line of the file's header, so the
    /// reader is in the file from its header on; the blank line above the
    /// header is still in the file before. With several files, the first
    /// entry is the first file, so the top of the document is in it, and
    /// `None` is only for a document whose first heading is below the top.
    pub fn current(&self, map: &Map, top: usize) -> Option<usize> {
        let shown = self
            .entries
            .partition_point(|entry| map.line_of(&entry.start).is_some_and(|line| line <= top));
        shown.checked_sub(1)
    }
}

#[cfg(test)]
mod tests;
